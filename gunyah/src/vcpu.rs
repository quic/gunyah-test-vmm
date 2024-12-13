// Copyright (c) 2024, Qualcomm Innovation Center, Inc. All rights reserved.
// SPDX-License-Identifier: BSD-3-Clause-Clear

use std::{
    fs::File,
    mem::size_of,
    os::fd::{AsRawFd, FromRawFd, RawFd},
};

use anyhow::{Context, Result};
use claim::assert_ge;
use same_file::Handle;

use gunyah_bindings::{gunyah_fn_vcpu_arg, gunyah_vcpu_mmap_size, gunyah_vcpu_run};
use memmap::{MmapMut, MmapOptions};

use crate::vm::{VcpuFunction, Vm};

#[derive(Debug)]
pub struct Vcpu {
    vm: Vm,
    id: u32,
    vcpu: Handle,
    mmap: MmapMut,
}

impl Vcpu {
    pub fn new(vm: Vm, id: u32) -> Result<Self> {
        let raw_fd = vm
            .add_function::<VcpuFunction>(&gunyah_fn_vcpu_arg { id })
            .context("failed to create vcpu with vm")?;

        // SAFETY: Safe because we know this is a Vcpu fd because only we can create Vcpu
        let mmap_size: usize = unsafe { gunyah_vcpu_mmap_size(raw_fd) }
            .context("failed to get vcpu mmap size")?
            .try_into()
            .unwrap();

        assert_ge!(mmap_size, size_of::<gunyah_vcpu_run>());

        // SAFETY: Safe because we created the Vcpu fd
        let vcpu = unsafe { File::from_raw_fd(raw_fd) };

        // SAFETY: Safe because the kernel told us what size to mmap the vcpu file
        let mmap = unsafe { MmapOptions::new().len(mmap_size).map_mut(&vcpu) }
            .context("failed to mmap vcpu")?;

        Ok(Self {
            vm,
            id,
            vcpu: Handle::from_file(vcpu).context("failed to stat vcpu")?,
            mmap,
        })
    }

    pub fn mmap(&self) -> &gunyah_vcpu_run {
        // SAFETY: Safe because we own the mmap and know it was mapped to a gunayh_vcpu_run struct
        unsafe { (self.mmap.as_ptr() as *const gunyah_vcpu_run).as_ref() }.unwrap()
    }

    pub fn mmap_mut(&mut self) -> &mut gunyah_vcpu_run {
        // SAFETY: Safe because we own the mmap and know it was mapped to a gunayh_vcpu_run struct
        unsafe { (self.mmap.as_mut_ptr() as *mut gunyah_vcpu_run).as_mut() }.unwrap()
    }

    pub fn run(&self) -> nix::Result<()> {
        // SAFETY: Safe because we know we are a vcpu fd
        unsafe { gunyah_vcpu_run(self.as_raw_fd()) }.map(|_| ())
    }

    pub fn id(&self) -> u32 {
        self.id
    }
}

impl Drop for Vcpu {
    fn drop(&mut self) {
        self.vm
            .remove_function::<VcpuFunction>(&gunyah_fn_vcpu_arg { id: self.id })
            .unwrap();
    }
}

impl AsRawFd for Vcpu {
    fn as_raw_fd(&self) -> RawFd {
        self.vcpu.as_raw_fd()
    }
}

impl PartialEq for Vcpu {
    fn eq(&self, other: &Self) -> bool {
        self.vcpu == other.vcpu
    }
}

#[cfg(test)]
mod tests {
    use claim::*;
    use gunyah_bindings::gunyah_vcpu_exit;

    use super::*;
    use crate::Gunyah;

    #[test]
    pub fn create_vcpu() {
        let gunyah = Gunyah::new().unwrap();
        let vm = gunyah.create_vm().unwrap();

        for _ in 0..2 {
            for id in 0..8 {
                assert_ok!(Vcpu::new(vm.clone(), id));
            }
        }
    }

    #[test]
    pub fn mmap() {
        let gunyah = Gunyah::new().unwrap();
        let vm = gunyah.create_vm().unwrap();

        let vcpu = Vcpu::new(vm, 0).unwrap();

        assert_eq!(
            vcpu.mmap().exit_reason,
            gunyah_vcpu_exit::GUNYAH_VCPU_EXIT_UNKNOWN
        );
    }

    #[test]
    pub fn drops() {
        let gunyah = Gunyah::new().unwrap();
        let vm = gunyah.create_vm().unwrap();

        assert_ok!(Vcpu::new(vm.clone(), 0));
        assert_ok!(Vcpu::new(vm.clone(), 0));
    }
}
