// Copyright (c) 2024, Qualcomm Innovation Center, Inc. All rights reserved.
// SPDX-License-Identifier: BSD-3-Clause-Clear

use std::cell::OnceCell;
use std::ffi::OsStr;
use std::fmt::Debug;
use std::io::Stdout;
use std::ops::Add;
use std::sync::{Arc, Mutex};

use std::{fs, io, thread};
use std::{path::PathBuf, str::FromStr};

use anyhow::{anyhow, Context, Result};
use clap::{ArgAction, Parser};
use gunyah::GuestMemoryAccess;
use gunyah_test_vmm::{GuestAddress, GuestSize, SerialDevice};
use vmm::{FdtWriter, GunyahVirtualMachine};

#[derive(Clone, Debug)]
struct LoadFileArg {
    file: PathBuf,
    addr: GuestAddress,
}

impl FromStr for LoadFileArg {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts = s.split(',');
        let file = PathBuf::from(parts.next().ok_or(anyhow!("No path specified"))?);
        let addr = GuestAddress::from_str(parts.next().ok_or(anyhow!("No address specified"))?)?;
        Ok(Self { file, addr })
    }
}

#[derive(Parser, Debug)]
/// Run a Gunyah Virtual Machine
struct RunCommand {
    /// Binary image to execute
    image: PathBuf,

    /// Base address of the binary image. If not specified, then use MEM_BASE.
    #[arg(long, short)]
    image_base: Option<GuestAddress>,

    // Ramdisk to be loaded
    rdisk: PathBuf,

    /// List of files to load into the VM memory
    #[arg(id = "FILE,ADDR")]
    files: Vec<LoadFileArg>,

    /// Base address of the VM's memory
    #[arg(long, short, default_value_t = 0x8000_0000u64.into())]
    mem_base: GuestAddress,

    /// Size of the VM's memory
    #[arg(long, short, default_value_t = GuestSize::from_str("100MB").unwrap())]
    size: GuestSize,

    /// Number of vCPUs to spawn
    #[arg(long, default_value_t = 8)]
    vcpus: u8,

    /// Address to place DTB configuration. If none, places at the end of guest memory
    #[arg(long)]
    dtb_base: Option<GuestAddress>,

    /// Use huge pages
    #[arg(long)]
    huge_pages: bool,

    /// Launches an unprotected VM where primary guest memory is shared instead of lent
    #[arg(long = "unprotected", default_value_t = true, action=ArgAction::SetFalse)]
    protected: bool,

    /// Additional kernel command line options
    #[arg(long = "cmdline", short, default_value_t=String::from("nokaslr earlycon console=ttyACM0 rw root=/dev/ram rdinit=/sbin/init console=ttyS0"))]
    command_line: String,

    /// GIC Distributor base address
    #[arg(long, default_value_t = 0x3FFF0000u64.into())]
    gic_dist_base: GuestAddress,
    /// GIC Distributor size
    #[arg(long, default_value_t = 0x10000u64.into())]
    gic_dist_size: GuestSize,
    /// GIC Redistributor base address. If none, redistributor is placed before the distributor.
    #[arg(long)]
    gic_redist_base: Option<GuestAddress>,
    /// GIC Redistributor size per CPU
    #[arg(long, default_value_t = 0x20000u64.into())]
    gic_redist_size: GuestSize,

    /// Serial port address
    #[arg(long, default_value_t = 0x3f800u64.into())]
    serial_base: GuestAddress,
    /// Serial port SPI
    #[arg(long, default_value_t = 1)]
    serial_interrupt: u32,
}

impl RunCommand {
    pub fn validate(&self) -> Result<()> {
        if !self.image.is_file() {
            return Err(anyhow!(format!("{} is not a file", self.image.display())));
        }

        if self.vcpus == 0 {
            return Err(anyhow!(
                "Need more than zero vCPUs to run a virtual machine"
            ));
        }

        if let Some(f) = self.files.iter().find(|f| !f.file.is_file()) {
            return Err(anyhow!(format!("{} is not a file", f.file.display())));
        }
        Ok(())
    }
}

struct Run {
    args: RunCommand,

    serial: Option<Arc<Mutex<SerialDevice<Stdout>>>>,
    vm: GunyahVirtualMachine,
    page_size_once: OnceCell<usize>,
}

impl Run {
    pub fn new(args: RunCommand) -> Result<Self> {
        Ok(Self {
            args,
            serial: None,
            page_size_once: OnceCell::new(),
            vm: GunyahVirtualMachine::new().context("Failed to create Gunyah Virtual Machine")?,
        })
    }

    fn mem_end(&self) -> GuestAddress {
        self.args.mem_base + self.args.size
    }

    fn page_size(&self) -> usize {
        *self.page_size_once.get_or_init(|| {
            if self.args.huge_pages {
                usize::from_str(
                    fs::read_to_string("/sys/kernel/mm/transparent_hugepage/hpage_pmd_size")
                        .unwrap()
                        .trim(),
                )
                .context("Failed to parse hpage_pmd_size")
                .unwrap()
            } else {
                page_size::get()
            }
        })
    }

    fn align_address_offset(&self, addr: GuestAddress, offset: u64) -> Result<GuestAddress> {
        Ok(((*addr + offset) & !offset).into())
    }

    fn align_address(&self, addr: GuestAddress) -> Result<GuestAddress> {
        let page_mask: u64 = (self.page_size() - 1).try_into()?;
        self.align_address_offset(addr, page_mask)
    }

    fn align_size(&self, size: GuestSize) -> Result<GuestSize> {
        let page_mask: u64 = (self.page_size() - 1).try_into()?;
        Ok(((*size + page_mask) & !page_mask).into())
    }

    fn load_binaries(&self) -> Result<()> {
        let image_base = self.args.image_base.unwrap_or(self.args.mem_base);
        let image = fs::read(&self.args.image).context("Unable to read VM image")?;

        let rdisk = fs::read(&self.args.rdisk).context("Unable to read Ramdisk image")?;
        let image_end = image_base.add(self.align_size((image.len() + self.page_size()).into())?);
        let rdisk_base = self.align_address_offset(image_end, 0x100_0000u64 - 1)?;

        let command_line = self.args.command_line.clone();
        let dtb = self.generate_fdt(
            &command_line,
            rdisk_base,
            rdisk_base.add(rdisk.len().into()),
        )?;
        let dtb_addr = match self.args.dtb_base {
            Some(b) => b,
            None => {
                let addr = self
                    .mem_end()
                    .checked_sub(dtb.len().try_into().unwrap())
                    .and_then(|a| a.checked_sub(self.page_size().try_into().unwrap()))
                    .and_then(|a| a.checked_sub(self.page_size().try_into().unwrap()))
                    .expect("Memory size should be large enough to contain DTB");
                self.align_address(addr.into())?
            }
        };
        let dtb_len = self.align_size((dtb.len() + self.page_size()).into())?;

        if !self.args.files.is_empty() {
            todo!();
        }

        let mut regions: Vec<(&OsStr, GuestAddress, GuestSize)> = Vec::new();
        regions.push((OsStr::new("dtb"), dtb_addr, dtb_len));
        regions.push((self.args.image.as_os_str(), image_base, image.len().into()));
        regions.push((self.args.rdisk.as_os_str(), rdisk_base, rdisk.len().into()));
        for arg in &self.args.files {
            regions.push((
                arg.file.as_os_str(),
                arg.addr,
                arg.file.metadata()?.len().into(),
            ))
        }

        regions.sort_by_key(|v| *v.1);
        if let Some(cell) = regions
            .windows(2)
            .find(|cell| cell[0].1 + cell[0].2 > cell[1].1)
        {
            return Err(anyhow!(format!(
                "{} ({}@{}) should not overlap with {} ({}@{})",
                cell[0].0.to_str().unwrap(),
                cell[0].2,
                cell[0].1,
                cell[1].0.to_str().unwrap(),
                cell[1].2,
                cell[1].1
            )));
        }

        if let Some(last) = regions.last() {
            if last.1 + last.2 > self.mem_end() {
                return Err(anyhow!(format!(
                    "{} ({}@{}/{}) should not lie outside memory ({}@{}/{})",
                    last.0.to_string_lossy(),
                    last.2,
                    last.1,
                    last.1 + last.2,
                    self.args.size,
                    self.args.mem_base,
                    self.mem_end()
                )));
            }
        }
        self.vm.set_dtb_config(*dtb_addr, *dtb_len, &dtb)?;
        self.vm.set_boot_pc(*image_base)?;

        self.vm
            .write_slice(*image_base, image.as_slice())
            .context("Unable to copy binary image to VM's memory")?;

        self.vm
            .write_slice(*rdisk_base, rdisk.as_slice())
            .context("Unable to copy ramdisk to VM's memory")?;

        Ok(())
    }

    fn generate_fdt(
        &self,
        command_line: &str,
        rdisk_start: GuestAddress,
        rdisk_end: GuestAddress,
    ) -> Result<Vec<u8>> {
        let mut fdt = FdtWriter::new()?;
        let root_node = fdt.begin_node("")?;

        self.vm.create_fdt_basic_config(
            &mut fdt,
            &[
                *self.args.gic_dist_base,
                *self.args.gic_dist_size,
                match self.args.gic_redist_base {
                    Some(b) => *b,
                    None => {
                        let offset = *self.args.gic_redist_size * u64::from(self.args.vcpus);
                        *self.args.gic_dist_base - offset
                    }
                },
                *self.args.gic_redist_size * u64::from(self.args.vcpus),
            ],
            &[13, 14, 11, 10], // TODO: move this to command line option
        )?;

        let chosen = fdt.begin_node("chosen")?;
        if let Some(ser) = &self.serial {
            fdt.property_string(
                "stdout-path",
                &format!("/{}", ser.lock().unwrap().device_name()),
            )?;
        }
        fdt.property_string("bootargs", command_line)?;

        fdt.property_u32("linux,initrd-start", *rdisk_start as u32)?;
        fdt.property_u32("linux,initrd-end", *rdisk_end as u32)?;

        fdt.end_node(chosen)?;

        fdt.end_node(root_node)?;

        fdt.finish().context("Failed to finalize dtb")
    }

    pub fn execute(mut self) -> Result<()> {
        self.args.validate()?;

        let vcpus = Arc::new(Mutex::new(Vec::new()));
        let mut vcpu_handles = Vec::new();

        for id in 0..self.args.vcpus {
            vcpus
                .lock()
                .unwrap()
                .push(self.vm.create_vcpu(id).context("Failed to create vcpu"));
        }

        self.serial = Some(SerialDevice::new(
            &mut self.vm,
            *self.args.serial_base,
            self.args.serial_interrupt,
            io::stdout(),
        )?);

        self.vm
            .add_memory(
                *self.args.mem_base,
                self.args.size.try_into()?,
                if self.args.protected {
                    gunyah::ShareType::Lend
                } else {
                    gunyah::ShareType::Share
                },
                GuestMemoryAccess::Rwx,
                self.args.huge_pages,
            )
            .expect("Failed to add memory to the vm");

        self.load_binaries()?;

        self.vm.start().context("Failed to start the VM")?;

        for _id in 0..self.args.vcpus {
            let vcpu = vcpus.lock().unwrap().pop().unwrap()?;
            vcpu_handles.push(thread::spawn(move || {
                vcpu.run().unwrap();
            }));
        }

        for _id in 0..self.args.vcpus {
            let handle = vcpu_handles.pop().unwrap();
            handle.join().unwrap();
        }

        Ok(())
    }
}

fn main() -> Result<()> {
    Run::new(RunCommand::parse())?.execute()
}
