// Copyright (c) 2024, Qualcomm Innovation Center, Inc. All rights reserved.
// SPDX-License-Identifier: BSD-3-Clause-Clear

use std::{num::NonZeroUsize, ops::DerefMut};

use gunyah::{GuestMemRegion, GuestMemoryAccess, ShareType, Vm};

use crate::{
    AccessId::{Vcpu, VmmUserspace},
    BusDevice,
};
use anyhow::{anyhow, Context, Result};

pub struct GunyahGuestMemoryRegion {
    region: GuestMemRegion,
    guest_address: u64,
    vm: Vm,
    share_type: ShareType,
    guest_access: GuestMemoryAccess,
    unmap_on_drop: bool,
    regular_memory: bool,
}

impl GunyahGuestMemoryRegion {
    pub fn new(
        region: GuestMemRegion,
        guest_address: u64,
        vm: &mut Vm,
        share_type: ShareType,
        guest_access: GuestMemoryAccess,
        unmap_on_drop: bool,
        regular_memory: bool,
    ) -> Result<Self> {
        vm.map_memory(guest_address, share_type, guest_access, &region)
            .context("Failed to map into the guest")?;
        Ok(Self {
            region,
            guest_address,
            vm: vm.clone(),
            share_type,
            guest_access,
            unmap_on_drop,
            regular_memory,
        })
    }

    pub fn as_region(&self) -> &GuestMemRegion {
        &self.region
    }

    pub fn guest_address(&self) -> u64 {
        self.guest_address
    }

    pub fn punch_hole(&mut self, offset: u64, len: usize) -> Result<Vec<GunyahGuestMemoryRegion>> {
        let mut vec = Vec::new();

        if offset != 0 {
            vec.push(Self {
                region: GuestMemRegion::new(
                    self.region.as_guest_mem().clone(),
                    self.region.offset(),
                    usize::try_from(offset)?.try_into()?,
                )?,
                guest_address: self.guest_address,
                vm: self.vm.dup()?,
                share_type: self.share_type,
                guest_access: self.guest_access,
                unmap_on_drop: self.unmap_on_drop,
                regular_memory: self.regular_memory,
            });
        }

        let end: u64 = offset + len as u64;
        let region_end: u64 = self.region.offset() + self.region.size() as u64;
        if end != region_end {
            vec.push(Self {
                region: GuestMemRegion::new(
                    self.region.as_guest_mem().clone(),
                    self.region.offset() + end,
                    usize::try_from(region_end - end)?.try_into()?,
                )?,
                guest_address: self.guest_address + end,
                vm: self.vm.dup()?,
                share_type: self.share_type,
                guest_access: self.guest_access,
                unmap_on_drop: self.unmap_on_drop,
                regular_memory: self.regular_memory,
            })
        }

        assert!(!vec.is_empty());

        self.vm.unmap_memory(
            self.guest_address + offset,
            self.share_type,
            self.guest_access,
            &self.region,
        )?;

        self.unmap_on_drop = false;

        Ok(vec)
    }
}

impl Drop for GunyahGuestMemoryRegion {
    fn drop(&mut self) {
        if self.unmap_on_drop {
            self.vm
                .unmap_memory(
                    self.guest_address,
                    self.share_type,
                    self.guest_access,
                    &self.region,
                )
                .expect("Unable to unmap guest memory");
        }
    }
}

impl BusDevice for GunyahGuestMemoryRegion {
    fn debug_label(&self) -> String {
        format!("GunyahGuestMemoryRegion@{:x}", self.guest_address)
    }

    fn read(&mut self, access: crate::BusAccessInfo, data: &mut [u8]) -> anyhow::Result<()> {
        match access.id {
            VmmUserspace => {
                let src = self.region.map_region(
                    access.offset,
                    NonZeroUsize::new(data.len()).ok_or(anyhow!("data length was zero"))?,
                )?;

                crate::unsafe_read::cautious_memcpy(data, &src)
                    .or(Err(anyhow!("unable to read memory")))?;

                Ok(())
            }
            Vcpu(_) => todo!(),
        }
    }

    fn write(&mut self, access: crate::BusAccessInfo, data: &[u8]) -> anyhow::Result<()> {
        match access.id {
            VmmUserspace => {
                let mut src = self.region.map_region_mut(
                    access.offset,
                    NonZeroUsize::new(data.len()).ok_or(anyhow!("data length was zero"))?,
                )?;
                crate::unsafe_read::cautious_memcpy(src.deref_mut(), data)
                    .or(Err(anyhow!("unable to write memory")))?;
                Ok(())
            }
            Vcpu(_) => todo!(),
        }
    }

    fn memory_regions(&self) -> Option<Box<[u64]>> {
        if self.regular_memory {
            Some(Box::new([
                self.guest_address,
                self.region
                    .size()
                    .try_into()
                    .expect("Unable to convert usize to u64"),
            ]))
        } else {
            None
        }
    }
}
