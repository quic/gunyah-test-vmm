// Copyright (c) 2024, Qualcomm Innovation Center, Inc. All rights reserved.
// SPDX-License-Identifier: BSD-3-Clause-Clear

use std::fs;
use std::str::FromStr;
use std::{
    num::NonZeroUsize,
    sync::{Arc, OnceLock},
};

use anyhow::{anyhow, bail, Context, Result};
use gunyah::{GuestMemoryAccess, ShareType};
use gunyah_bindings::{gunyah_vcpu_exit::GUNYAH_VCPU_EXIT_MMIO, gunyah_vcpu_run};
use modular_bitfield::{
    bitfield,
    specifiers::{B4, B47, B8},
};
use pow2::Pow2;
use vm_fdt::FdtWriter;
use vmm::{GunyahVcpu, GunyahVirtualMachine};

macro_rules! kib {
    ($x:expr) => {
        $x * 1024
    };
}

macro_rules! mib {
    ($x:expr) => {
        $x * 1048576
    };
}

macro_rules! gunyah_hvc {
    ($x:expr) => {
        ((1 << 31) | (1 << 30) | ((6 & 0x3f) << 24) | ($x & 0xffff))
    };
}

pub const HOLDING_CELL_BIN: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/holding-cell.bin"));

pub struct HoldingCell {
    pub vm: GunyahVirtualMachine,
    pub vcpus: Vec<Arc<GunyahVcpu>>,
}

fn generate_holding_cell_fdt(vm: &GunyahVirtualMachine, num_cells: u8) -> Result<Vec<u8>> {
    let mut fdt = FdtWriter::new()?;
    let root_node = fdt.begin_node("")?;

    let gic_dist_base = 0x3FFF0000;
    let gic_redist_size = 0x20000 * num_cells as u64;
    let gic_redist_base = gic_dist_base - gic_redist_size;

    vm.create_fdt_basic_config(
        &mut fdt,
        &[gic_dist_base, 0x10000, gic_redist_base, gic_redist_size],
        &[13, 14, 11, 10],
    )?;
    fdt.end_node(root_node)?;
    Ok(fdt.finish()?)
}

#[bitfield]
#[allow(dead_code)]
struct Command {
    command: B8,
    nargs: B4,
    #[skip]
    __: B4,
    hold: bool,
    #[skip]
    ___: B47,
}

#[derive(PartialEq, Eq)]
pub enum FlushType {
    FlushEvery,
    FlushAfter,
    FlushOnLast,
    NoFlush,
}

macro_rules! punch_hole {
    ($x:expr, $off:expr, $len:expr) => {
        $x.lock()
            .unwrap()
            .as_region()
            .as_guest_mem()
            .punch_hole($off, $len)
    };
}

pub struct HoldingCellOptions {
    num_cells: u8,
    huge_pages: bool,
}

impl Default for HoldingCellOptions {
    fn default() -> Self {
        Self {
            num_cells: 1,
            huge_pages: Default::default(),
        }
    }
}

fn page_size(huge: bool) -> Pow2 {
    static PAGE_SIZE_ONCE: OnceLock<usize> = OnceLock::new();
    Pow2::try_from(if huge {
        *PAGE_SIZE_ONCE.get_or_init(|| {
            usize::from_str(
                fs::read_to_string("/sys/kernel/mm/transparent_hugepage/hpage_pmd_size")
                    .unwrap()
                    .trim(),
            )
            .context("Failed to parse hpage_pmd_size")
            .unwrap()
        })
    } else {
        page_size::get()
    })
    .expect("Page size not a power of 2?")
}

fn holding_cell_rounded_size() -> usize {
    static SIZE_ONCE: OnceLock<usize> = OnceLock::new();
    *SIZE_ONCE.get_or_init(|| {
        page_size(false)
            .align_up(HOLDING_CELL_BIN.len())
            .expect("holding cell binary too big")
    })
}

/// Holding Cell Memory Map, starts at 0x8000_0000 and all the entries are page-aligned
/// Stack size is 1 page (4kb)
/// [binary][dtb][cpu0 stack][cpuN stack...]

impl HoldingCell {
    pub fn new_with_options(options: HoldingCellOptions) -> Self {
        // Hard-coded at 8000_0000 because I can't find an elf loader to and do the relocations
        let start_addr: u64 = 0x8000_0000;
        let dtb_start = start_addr + holding_cell_rounded_size() as u64;

        // Memory for the binary + 1 page for DTB + 1 page for each cpu's stack
        let mem_size =
            holding_cell_rounded_size() + (usize::from(1 + options.num_cells) * page_size(false));
        let mem_size = page_size(options.huge_pages)
            .align_up(mem_size)
            .expect("memory size too big");
        let mem_size = NonZeroUsize::new(mem_size).unwrap();

        let mut vm = GunyahVirtualMachine::new().expect("Failed to create Gunyah Virtual machine");
        vm.add_memory(
            start_addr,
            mem_size,
            ShareType::Lend,
            GuestMemoryAccess::Rwx,
            options.huge_pages,
        )
        .expect("Failed to add memory to the vm");
        let mut vcpus = Vec::new();
        for id in 0..options.num_cells {
            vcpus.push(vm.create_vcpu(id).expect("Failed to create vcpu"));
        }

        let dtb = generate_holding_cell_fdt(&vm, options.num_cells)
            .expect("Failed to generate holding cell DT");
        vm.set_dtb_config(
            dtb_start,
            page_size(false).align_up(dtb.len()).expect("dtb too big") as u64,
            &dtb,
        )
        .expect("Failed to set dtb configuration");

        vm.write_slice(start_addr, HOLDING_CELL_BIN)
            .expect("Failed to copy binary image to VM's memory");
        vm.set_boot_pc(start_addr).expect("Failed to set boot pc");
        vm.set_boot_sp(dtb_start + kib!(8))
            .expect("Failed to set boot sp");

        Self { vm, vcpus }
    }

    pub fn new() -> Self {
        Self::new_with_options(Default::default())
    }

    fn test_errors(vcpu: &GunyahVcpu) -> Result<()> {
        let result = vcpu.status();
        if result.exit_reason == GUNYAH_VCPU_EXIT_MMIO {
            // SAFETY: Safe because we just checked exit reason is EXIT_MMIO
            let mmio = unsafe { result.__bindgen_anon_1.mmio };

            if mmio.phys_addr == 0x7000 {
                let esr = u64::from_le_bytes(mmio.data);
                let result = vcpu
                    .run_once()
                    .context(format!("Failed to read FAR after getting ESR={:x}", esr))?;
                assert_eq!(result.exit_reason, GUNYAH_VCPU_EXIT_MMIO);
                // SAFETY: Safe because we just checked exit reason is EXIT_MMIO
                let mmio = unsafe { result.__bindgen_anon_1.mmio };
                assert_eq!(mmio.phys_addr, 0x7000);
                let far = u64::from_le_bytes(mmio.data);
                bail!("holding cell got sync abort. esr={:x} far={:x}", esr, far);
            }
        }

        Ok(())
    }

    pub fn run_test(
        &self,
        cell_id: u8,
        test: u8,
        args: &[u64],
        hold: bool,
    ) -> Result<Box<dyn Fn() -> Result<u64> + '_>> {
        self.vm.start().context("Failed to start vcpu")?;
        let vcpu = &self.vcpus[cell_id as usize];
        vcpu.run_once()
            .context("Failed to run vcpu before providing command")?;
        Self::test_errors(vcpu)?;
        let command = Command::new()
            .with_command(test)
            .with_nargs(args.len().try_into()?)
            .with_hold(hold)
            .into_bytes();
        vcpu.vmmio_provide_read(0x6000, &command)
            .context(format!("Failed to provide command: {:?}", vcpu.status()))?;

        for arg in args {
            vcpu.run_once()
                .context(format!("Failed to run vcpu before providing {arg}"))?;
            Self::test_errors(vcpu)?;
            vcpu.vmmio_provide_read(0x6000, &arg.to_le_bytes())?;
        }

        if hold {
            Ok(Box::new(|| {
                let result = vcpu
                    .run_once()
                    .context("Failed to run vcpu to get result")?;
                Self::test_errors(vcpu)?;
                if result.exit_reason != GUNYAH_VCPU_EXIT_MMIO {
                    bail!("unexpected exit reason: {:?}", result)
                }
                // SAFETY: Safe because we just checked exit reason is EXIT_MMIO
                let mmio = unsafe { result.__bindgen_anon_1.mmio };
                if mmio.phys_addr != 0x6000 || mmio.is_write != 1 {
                    bail!("unexpected mmio exit reason: {:?}", mmio)
                }
                Ok(u64::from_le_bytes(mmio.data))
            }))
        } else {
            let result = vcpu
                .run_once()
                .context("Failed to run vcpu to get result")?;
            Self::test_errors(vcpu)?;
            if result.exit_reason != GUNYAH_VCPU_EXIT_MMIO {
                bail!("unexpected exit reason: {:?}", result)
            }
            // SAFETY: Safe because we just checked exit reason is EXIT_MMIO
            let mmio = unsafe { result.__bindgen_anon_1.mmio };
            if mmio.phys_addr != 0x6000 || mmio.is_write != 1 {
                bail!("unexpected mmio exit reason: {:?}", mmio)
            }
            Ok(Box::new(move || Ok(u64::from_le_bytes(mmio.data))))
        }
    }

    pub fn run_immediately(&self, cell_id: u8, test: u8, args: &[u64]) -> Result<u64> {
        self.run_test(cell_id, test, args, false).and_then(|f| f())
    }

    pub fn ack_ok(&self, cell_id: u8) -> Result<()> {
        self.run_immediately(cell_id, 0, &[])?;
        Ok(())
    }

    pub fn read_addr(&self, cell_id: u8, addr: u64) -> Result<u64> {
        self.run_immediately(cell_id, 2, &[addr])
    }

    pub fn write_addr(&self, cell_id: u8, addr: u64, value: u64) -> Result<()> {
        if self.run_immediately(cell_id, 3, &[addr, value])? != 0 {
            Err(anyhow!("Unexpected nonzero response"))
        } else {
            Ok(())
        }
    }

    pub fn read_io(&self, cell_id: u8, addr: u64, value: u64) -> Result<u64> {
        self.vm.start().context("Failed to start vcpu")?;
        let vcpu = &self.vcpus[cell_id as usize];
        vcpu.run_once()
            .context("Failed to run vcpu before providing command")?;
        Self::test_errors(vcpu)?;
        let command = Command::new().with_command(8).with_nargs(1).into_bytes();
        vcpu.vmmio_provide_read(0x6000, &command)
            .context(format!("Failed to provide command: {:?}", vcpu.status()))?;

        vcpu.run_once()
            .context("Failed to run vcpu before providing addr")?;
        Self::test_errors(vcpu)?;
        vcpu.vmmio_provide_read(0x6000, &addr.to_le_bytes())?;

        vcpu.run_once()
            .context("Failed to run vcpu before providing value")?;
        Self::test_errors(vcpu)?;

        vcpu.vmmio_provide_read(addr, &value.to_le_bytes())?;

        let result = vcpu
            .run_once()
            .context("Failed to run vcpu after providing value")?;
        Self::test_errors(vcpu)?;

        if result.exit_reason != GUNYAH_VCPU_EXIT_MMIO {
            bail!("unexpected exit reason: {:?}", result)
        }
        // SAFETY: Safe because we just checked exit reason is EXIT_MMIO
        let mmio = unsafe { result.__bindgen_anon_1.mmio };
        if mmio.phys_addr != 0x6000 || mmio.is_write != 1 {
            bail!("unexpected mmio exit reason: {:?}", mmio)
        }
        Ok(u64::from_le_bytes(mmio.data))
    }

    pub fn write_io(&self, cell_id: u8, addr: u64, value: u64) -> Result<()> {
        if self.run_immediately(cell_id, 9, &[addr, value])? != 0 {
            Err(anyhow!("Unexpected nonzero response"))
        } else {
            Ok(())
        }
    }

    pub fn smccc_immediately(&self, cell_id: u8, args: &[u64]) -> Result<u64> {
        let mut _args = [0u64; 5];
        _args[..args.len()].copy_from_slice(args);
        self.run_immediately(cell_id, 6, &_args)
    }

    pub fn power_on_cell(&self, cell_id: u8) -> Result<()> {
        self.smccc_immediately(
            0,
            &[
                0xC400_0003,
                self.vcpus[cell_id as usize].id() as u64,
                0x8000_0000,
                0,
                0,
            ],
        )
        .map(|_| ())
    }

    pub fn power_off(&self, cell_id: u8) -> Result<()> {
        if self.smccc_immediately(cell_id, &[0x8400_0008]).is_err() {
            Ok(())
        } else {
            Err(anyhow!("Failed to shutdown VM"))
        }
    }

    pub fn page_relinquish(
        &self,
        cell_id: u8,
        addr: u64,
        nr_pages: u32,
        sanitize: bool,
        flush: FlushType,
    ) -> Result<()> {
        let addrspc_flags = 0b1 | if sanitize { 0b10 } else { 0 };
        for i in 1..(nr_pages + 1) {
            let flags = addrspc_flags
                | match flush {
                    FlushType::FlushEvery => 0b100,
                    FlushType::FlushOnLast => {
                        if i == nr_pages {
                            0b100
                        } else {
                            0
                        }
                    }
                    FlushType::FlushAfter => 0,
                    FlushType::NoFlush => 0,
                };
            self.smccc_immediately(
                cell_id,
                &[
                    gunyah_hvc!(0x8069),
                    0,
                    addr + ((i - 1) * kib!(4)) as u64,
                    kib!(4),
                    flags,
                ],
            )?;
        }
        if flush == FlushType::FlushAfter {
            let ret = self.smccc_immediately(cell_id, &[gunyah_hvc!(0x8069), 0, 0, 0, 0b100])?;
            if ret != 0 {
                return Err(anyhow!("hypercall returned error: {}", ret));
            };
        }
        Ok(())
    }

    pub fn cell_state(&self, cell_id: u8) -> gunyah_vcpu_run {
        self.vcpus[cell_id as usize].status()
    }

    pub fn host_write_slice(&self, address: u64, data: &[u8]) -> Result<()> {
        self.vm.write_slice(address, data)
    }

    pub fn host_read_slice(&self, address: u64, data: &mut [u8]) -> Result<()> {
        self.vm.read_slice(address, data)
    }
}

mod basic;
mod ioevent;
mod memory;
mod multicore;
