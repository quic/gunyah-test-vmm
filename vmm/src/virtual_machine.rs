// Copyright (c) 2024, Qualcomm Innovation Center, Inc. All rights reserved.
// SPDX-License-Identifier: BSD-3-Clause-Clear

use std::{
    num::NonZeroUsize,
    sync::{Arc, Mutex, RwLock},
};

use anyhow::{Context, Result};
use gunyah::{GuestMemRegion, GuestMemoryAccess, Gunyah, Ioeventfd, ShareType};

use vm_fdt::FdtWriter;

use crate::{
    AccessId, Bus, BusDevice, BusDeviceSync, GunyahGuestMemoryRegion, GunyahInterrupt, GunyahVcpu,
};

pub struct GunyahVirtualMachine {
    vm: gunyah::Vm,
    vcpus: RwLock<Vec<Arc<GunyahVcpu>>>,
    bus: Bus,
    interrupts: RwLock<Vec<Arc<GunyahInterrupt>>>,
}

impl From<gunyah::Vm> for GunyahVirtualMachine {
    fn from(vm: gunyah::Vm) -> Self {
        Self {
            vm,
            vcpus: RwLock::new(Vec::new()),
            bus: Bus::new(),
            interrupts: RwLock::new(Vec::new()),
        }
    }
}

impl GunyahVirtualMachine {
    pub fn new() -> Result<Self> {
        Ok(gunyah::Gunyah::new()
            .context("Failed to open gunyah")?
            .create_vm()
            .context("Failed to create vm")?
            .into())
    }

    pub fn get_bus(&self, access: AccessId) -> Bus {
        self.bus.clone().set_access_id(access)
    }

    pub fn create_vcpu(&self, id: u8) -> Result<Arc<GunyahVcpu>> {
        let vcpu = Arc::new(GunyahVcpu::new(self, id).context("Failed to create vcpu")?);
        self.vcpus.write().unwrap().push(vcpu.clone());
        Ok(vcpu)
    }

    pub fn write_slice(&self, address: u64, data: &[u8]) -> Result<()> {
        self.bus.write(address, data)
    }

    pub fn read_slice(&self, address: u64, data: &mut [u8]) -> Result<()> {
        self.bus.read(address, data)
    }

    pub fn add_memory_region(
        &mut self,
        region: GuestMemRegion,
        guest_address: u64,
        share_type: ShareType,
        guest_access: GuestMemoryAccess,
        unmap_on_drop: bool,
        regular_memory: bool,
    ) -> Result<Arc<Mutex<GunyahGuestMemoryRegion>>> {
        let guest_region = Arc::new(Mutex::new(
            GunyahGuestMemoryRegion::new(
                region.clone(),
                guest_address,
                &mut self.vm,
                share_type,
                guest_access,
                unmap_on_drop,
                regular_memory,
            )
            .context("Failed to add guest memory region to vm")?,
        ));
        self.bus.insert(
            guest_region.clone(),
            guest_address,
            region.size().try_into()?,
        )?;
        Ok(guest_region)
    }

    pub fn add_memory(
        &mut self,
        start: u64,
        len: NonZeroUsize,
        share_type: ShareType,
        guest_access: GuestMemoryAccess,
        huge_pages: bool,
    ) -> Result<Arc<Mutex<GunyahGuestMemoryRegion>>> {
        let guest_mem = Gunyah::new()?
            .create_guest_memory(len, huge_pages)
            .context("Failed to create guest memory")?;
        let region = GuestMemRegion::new(guest_mem, 0, len)?;
        let regular_memory = match share_type {
            ShareType::Share => false,
            ShareType::Lend => true,
        };
        self.add_memory_region(
            region,
            start,
            share_type,
            guest_access,
            false,
            regular_memory,
        )
    }

    pub fn add_regular_memory(
        &mut self,
        start: u64,
        len: NonZeroUsize,
        share_type: ShareType,
        guest_access: GuestMemoryAccess,
        huge_pages: bool,
    ) -> Result<Arc<Mutex<GunyahGuestMemoryRegion>>> {
        let guest_mem = Gunyah::new()?.create_guest_memory(len, huge_pages)?;
        let region = GuestMemRegion::new(guest_mem, 0, len)?;
        self.add_memory_region(region, start, share_type, guest_access, false, true)
    }

    pub fn punch_hole(
        &self,
        region: Arc<Mutex<GunyahGuestMemoryRegion>>,
        offset: u64,
        len: usize,
    ) -> Result<()> {
        let mut region = region.lock().unwrap();

        let new_regions = region.punch_hole(offset, len)?;

        self.bus
            .remove(region.guest_address(), region.as_region().size() as u64)
            .expect("Failed to remove original region from VMM's bus");

        for new_region in new_regions {
            let guest_address = new_region.guest_address();
            let size = new_region.as_region().size() as u64;
            self.bus
                .insert(Arc::new(Mutex::new(new_region)), guest_address, size)
                .expect("Failed to insert replacement region into VMM's bus");
        }
        Ok(())
    }

    pub fn set_dtb_config(&self, start: u64, len: u64, dtb: &[u8]) -> Result<()> {
        self.write_slice(start, dtb)
            .context("Failed to copy DTB to VM")?;
        self.vm
            .set_dtb_config(start, len)
            .context("Failed to set DTB configuration for VM")
    }

    pub fn set_boot_pc(&self, value: u64) -> Result<(), gunyah::Error> {
        self.vm.set_boot_pc(value)
    }

    pub fn set_boot_sp(&self, value: u64) -> Result<(), gunyah::Error> {
        self.vm.set_boot_sp(value)
    }

    pub fn add_level_interrupt(&self, line: u32) -> Result<Arc<GunyahInterrupt>> {
        let interrupt: Arc<GunyahInterrupt> = Arc::new(
            GunyahInterrupt::new_level(self, line)
                .context(format!("Failed to create interrupt {}", line))?,
        );
        self.interrupts.write().unwrap().push(interrupt.clone());
        Ok(interrupt)
    }

    pub fn add_edge_interrupt(&self, line: u32) -> Result<Arc<GunyahInterrupt>> {
        let interrupt = Arc::new(
            GunyahInterrupt::new_edge(self, line)
                .context(format!("Failed to create interrupt {}", line))?,
        );
        self.interrupts.write().unwrap().push(interrupt.clone());
        Ok(interrupt)
    }

    pub fn add_ioevent(&self, addr: u64, len: u32, datamatch: Option<u64>) -> Result<Ioeventfd> {
        Ioeventfd::new(self.vm.clone(), addr, len, datamatch)
    }

    pub fn add_device(
        &mut self,
        device: Arc<Mutex<dyn BusDevice>>,
        base: u64,
        len: u64,
    ) -> Result<()> {
        self.bus.insert(device, base, len)?;
        Ok(())
    }

    pub fn add_device_sync(
        &mut self,
        device: Arc<dyn BusDeviceSync>,
        base: u64,
        len: u64,
    ) -> Result<()> {
        self.bus.insert_sync(device, base, len)?;
        Ok(())
    }

    pub fn start(&self) -> Result<(), gunyah::Error> {
        self.vm.start()
    }

    pub fn create_fdt_vm_config(
        &self,
        fdt: &mut FdtWriter,
        os_type: &str,
        base_address: u64,
        firmware_address: Option<u64>,
        intc_phandle: u32,
    ) -> Result<()> {
        let vm_config = fdt.begin_node("gunyah-vm-config")?;

        fdt.property_string("image-name", "gunyah-vmm-vm")?;
        fdt.property_string("os-type", os_type)?;

        let memory_node = fdt.begin_node("memory")?;
        fdt.property_u32("#address-cells", 2)?;
        fdt.property_u32("#size-cells", 2)?;
        fdt.property_u64("base-address", base_address)?;
        if let Some(addr) = firmware_address {
            fdt.property_u64("firmware-address", addr)?;
        }
        fdt.end_node(memory_node)?;

        let interrupts_node = fdt.begin_node("interrupts")?;
        fdt.property_u32("config", intc_phandle)?;
        fdt.end_node(interrupts_node)?;

        let vcpus_node = fdt.begin_node("vcpus")?;
        fdt.property_string("affinity", "proxy")?;
        fdt.end_node(vcpus_node)?;

        let vdev_node = fdt.begin_node("vdevices")?;
        fdt.property_string("generate", "/hypervisor")?;
        self.bus.generate_gunyah_vdevice_config(fdt)?;
        for interrupt in self.interrupts.read().unwrap().iter() {
            interrupt.generate_vdevice(fdt)?;
        }
        fdt.end_node(vdev_node)?;
        fdt.end_node(vm_config)?;
        Ok(())
    }

    pub fn create_fdt_basic_config(
        &self,
        fdt: &mut FdtWriter,
        gic_config: &[u64; 4],
        timer_interrupts: &[u32; 4],
    ) -> Result<()> {
        const PHANDLE_GIC: u32 = 1;

        fdt.property_u32("#address-cells", 2)?;
        fdt.property_u32("#size-cells", 2)?;
        fdt.property_u32("interrupt-parent", PHANDLE_GIC)?;

        let memory_node = fdt.begin_node("memory")?;
        fdt.property_string("device_type", "memory")?;
        let mem_reg = self.bus.list_memory_regions();
        fdt.property_array_u64("reg", &mem_reg)?;
        fdt.end_node(memory_node)?;

        let cpus_node = fdt.begin_node("cpus")?;
        fdt.property_u32("#address-cells", 1)?;
        fdt.property_u32("#size-cells", 0)?;
        for vcpu in self.vcpus.read().expect("Unable to read lock vcpus").iter() {
            let cpu_node = fdt.begin_node(&format!("cpu@{:x}", vcpu.id()))?;
            fdt.property_string("device_type", "cpu")?;
            fdt.property_string("compatible", "arm,arm-v8")?;
            fdt.property_string("enable-method", "psci")?;
            fdt.property_u32("reg", vcpu.id())?;
            // HACK: Force RM to set up PSCI
            fdt.property_null("cpu-idle-states")?;
            fdt.end_node(cpu_node)?;
        }
        fdt.end_node(cpus_node)?;

        let psci_node = fdt.begin_node("psci")?;
        fdt.property_string("compatible", "arm,psci-0.2")?;
        fdt.property_string("method", "hvc")?;
        fdt.end_node(psci_node)?;

        let intc_node = fdt.begin_node(&format!("interrupt-controller@{:x}", gic_config[0]))?;
        fdt.property_string("compatible", "arm,gic-v3")?;
        fdt.property_u32("#interrupt-cells", 3)?;
        fdt.property_u32("#address-cells", 2)?;
        fdt.property_u32("#size-cells", 2)?;
        fdt.property_null("interrupt-controller")?;
        fdt.property_array_u64("reg", gic_config)?;
        fdt.property_u32("phandle", PHANDLE_GIC)?;
        fdt.end_node(intc_node)?;

        let timer_node = fdt.begin_node("timer")?;
        fdt.property_string("compatible", "arm,armv8-timer")?;
        fdt.property_null("always-on")?;
        let interrupts: [u32; 12] = [
            1,
            timer_interrupts[0],
            0x108,
            1,
            timer_interrupts[1],
            0x108,
            1,
            timer_interrupts[2],
            0x108,
            1,
            timer_interrupts[3],
            0x108,
        ];
        fdt.property_array_u32("interrupts", &interrupts)?;
        fdt.property_u32("clock-frequency", 19200000)?;
        fdt.end_node(timer_node)?;

        self.bus.generate_device_config(fdt)?;

        self.create_fdt_vm_config(
            fdt,
            "linux",
            *mem_reg.first().expect("vm has no memory"),
            None,
            PHANDLE_GIC,
        )?;

        Ok(())
    }

    pub(crate) fn vm(&self) -> &gunyah::Vm {
        &self.vm
    }
}
