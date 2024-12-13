// Copyright (c) 2024, Qualcomm Innovation Center, Inc. All rights reserved.
// SPDX-License-Identifier: BSD-3-Clause-Clear

use std::sync::RwLock;

use anyhow::{anyhow, Result};
use gunyah_bindings::{
    gunyah_vcpu_exit::{
        GUNYAH_VCPU_EXIT_MMIO, GUNYAH_VCPU_EXIT_PAGE_FAULT, GUNYAH_VCPU_EXIT_STATUS,
        GUNYAH_VCPU_EXIT_UNKNOWN,
    },
    gunyah_vcpu_resume_action::{GUNYAH_VCPU_RESUME_FAULT, GUNYAH_VCPU_RESUME_HANDLED},
    gunyah_vcpu_run,
};

use crate::{Bus, GunyahVirtualMachine};

pub struct GunyahVcpu {
    bus: Bus,
    vcpu: RwLock<gunyah::Vcpu>,
}

impl GunyahVcpu {
    pub(crate) fn new(vm: &GunyahVirtualMachine, id: u8) -> Result<Self> {
        Ok(Self {
            bus: vm.get_bus(crate::AccessId::Vcpu(id)),
            vcpu: RwLock::new(gunyah::Vcpu::new(vm.vm().clone(), id.into())?),
        })
    }

    pub fn id(&self) -> u32 {
        self.vcpu.read().unwrap().id()
    }

    pub fn run_once(&self) -> Result<gunyah_vcpu_run> {
        let vcpu = self.vcpu.write().unwrap();
        vcpu.run()?;
        Ok(*vcpu.mmap())
    }

    pub fn run(&self) -> Result<()> {
        loop {
            let mut vcpu = self.vcpu.write().unwrap();
            vcpu.run()?;
            let result = vcpu.mmap_mut();
            match result.exit_reason {
                GUNYAH_VCPU_EXIT_UNKNOWN => Err(anyhow!("Unexpected exit for unknown reason")),
                GUNYAH_VCPU_EXIT_MMIO => {
                    // SAFETY: Safe because we just checked exit_reason is GUNYAH_VCPU_EXIT_MMIO and we are the only ones that run the vcpu
                    let reason = unsafe { &mut result.__bindgen_anon_1.mmio };
                    let len = reason.len as usize;
                    let handled = match reason.is_write {
                        1 => self.bus.write(reason.phys_addr, &reason.data[0..len]),
                        0 => self.bus.read(reason.phys_addr, &mut reason.data[0..len]),
                        _ => unreachable!(),
                    };
                    reason.resume_action = match handled {
                        Ok(_) => GUNYAH_VCPU_RESUME_HANDLED,
                        Err(e) => {
                            println!(
                                "Failed to handle address access at  {}: {:?}",
                                reason.phys_addr, e
                            );
                            GUNYAH_VCPU_RESUME_FAULT
                        }
                    }
                    .try_into()
                    .unwrap();
                    Ok(())
                }
                GUNYAH_VCPU_EXIT_STATUS => todo!(),
                GUNYAH_VCPU_EXIT_PAGE_FAULT => {
                    // SAFETY: Safe because we just checked exit_reason is GUNYAH_VCPU_EXIT_PAGE_FAULT and we are the only ones that run the vcpu
                    let reason = unsafe { result.__bindgen_anon_1.page_fault };
                    Err(anyhow!(format!(
                        "Unexpected page fault at {:x}",
                        reason.phys_addr
                    )))
                }
                e => Err(anyhow!(format!("Unknown exit reason: {}", e))),
            }?;
        }
    }

    pub fn vmmio_provide_read(&self, phys_addr: u64, data: &[u8]) -> Result<()> {
        let mut vcpu = self.vcpu.write().unwrap();
        let result = vcpu.mmap_mut();
        if result.exit_reason != GUNYAH_VCPU_EXIT_MMIO {
            return Err(anyhow!("vCPU didn't exit for mmio"));
        }
        // SAFETY: Safe because we just checked exit_reason is GUNYAH_VCPU_EXIT_MMIO and we are the only ones that run the vcpu
        let reason = unsafe { &mut result.__bindgen_anon_1.mmio };
        if reason.is_write != 0 {
            return Err(anyhow!("vCPU didn't exit for mmio read"));
        }
        if reason.phys_addr != phys_addr {
            return Err(anyhow!(format!(
                "vCPU didn't exit for mmio read at {}",
                phys_addr
            )));
        }
        if reason.len as usize != data.len() {
            return Err(anyhow!("vCPU length didn't match"));
        }

        reason.data.copy_from_slice(data);
        reason.resume_action = GUNYAH_VCPU_RESUME_HANDLED as u8;

        Ok(())
    }

    pub fn status(&self) -> gunyah_vcpu_run {
        let vcpu = self.vcpu.read().unwrap();
        *vcpu.mmap()
    }
}
