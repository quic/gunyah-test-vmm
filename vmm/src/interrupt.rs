// Copyright (c) 2024, Qualcomm Innovation Center, Inc. All rights reserved.
// SPDX-License-Identifier: BSD-3-Clause-Clear

use anyhow::Result;
use gunyah::Irqfd;

use crate::GunyahVirtualMachine;

const GIC_FDT_IRQ_TYPE_SPI: u32 = 0;

const IRQ_TYPE_EDGE_RISING: u32 = 0x00000001;
const IRQ_TYPE_LEVEL_HIGH: u32 = 0x00000004;

#[derive(Debug)]
pub struct GunyahInterrupt {
    line: u32,
    irqfd: Irqfd,
}

impl GunyahInterrupt {
    pub(crate) fn new_level(vm: &GunyahVirtualMachine, line: u32) -> Result<Self> {
        Ok(Self {
            line,
            irqfd: Irqfd::new(vm.vm().clone(), line, true)?,
        })
    }

    pub(crate) fn new_edge(vm: &GunyahVirtualMachine, line: u32) -> Result<Self> {
        Ok(Self {
            line,
            irqfd: Irqfd::new(vm.vm().clone(), line, false)?,
        })
    }

    pub fn trigger(&self) -> Result<()> {
        self.irqfd.trigger()
    }

    pub fn line(&self) -> u32 {
        self.line
    }

    pub fn fdt_config(&self) -> [u32; 3] {
        [
            GIC_FDT_IRQ_TYPE_SPI,
            self.line(),
            if self.irqfd.level() {
                IRQ_TYPE_LEVEL_HIGH
            } else {
                IRQ_TYPE_EDGE_RISING
            },
        ]
    }

    pub(crate) fn generate_vdevice(
        &self,
        fdt: &mut vm_fdt::FdtWriter,
    ) -> Result<(), vm_fdt::Error> {
        let bell_name = format!("bell-{:x}", self.line);
        let bell_node = fdt.begin_node(&bell_name)?;
        fdt.property_string("vdevice-type", "doorbell")?;
        let path_name = format!("/hypervisor/bell-{:x}", self.line);
        fdt.property_string("generate", &path_name)?;
        fdt.property_u32("label", self.irqfd.label())?;
        fdt.property_null("peer-default")?;
        fdt.property_null("source-can-clean")?;
        fdt.property_array_u32("interrupts", &self.fdt_config())?;
        fdt.end_node(bell_node)?;

        Ok(())
    }
}
