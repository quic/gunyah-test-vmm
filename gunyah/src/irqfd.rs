// Copyright (c) 2024, Qualcomm Innovation Center, Inc. All rights reserved.
// SPDX-License-Identifier: BSD-3-Clause-Clear

use std::{
    fs::File,
    mem,
    os::fd::{AsRawFd, FromRawFd},
};

use anyhow::{Context, Result};
use gunyah_bindings::{gunyah_fn_irqfd_arg, gunyah_irqfd_flags};
use libc::c_void;
use nix::sys::eventfd::{eventfd, EfdFlags};
use same_file::Handle;

use crate::{IrqfdFunction, Vm};

#[derive(Debug)]
pub struct Irqfd {
    vm: Vm,
    label: u32,
    level: bool,
    eventfd: Handle,
}

impl Irqfd {
    pub fn new(vm: Vm, label: u32, level: bool) -> Result<Self> {
        let mut flags = 0;

        if level {
            flags |= gunyah_irqfd_flags::GUNYAH_IRQFD_FLAGS_LEVEL;
        }

        let raw_fd = eventfd(0, EfdFlags::empty()).context("failed to create eventfd")?;
        // SAFETY: Safe because we created the eventfd
        let eventfd = unsafe { File::from_raw_fd(raw_fd) };
        let handle = Handle::from_file(eventfd).context("failed to stat eventfd")?;

        assert!(
            vm.add_function::<IrqfdFunction>(&gunyah_fn_irqfd_arg {
                fd: raw_fd as u32,
                label,
                flags,
                ..Default::default()
            })
            .context("failed to register irqfd with vm")?
                == 0
        );

        Ok(Self {
            vm,
            label,
            level,
            eventfd: handle,
        })
    }

    pub fn label(&self) -> u32 {
        self.label
    }

    pub fn level(&self) -> bool {
        self.level
    }

    pub fn trigger(&self) -> Result<()> {
        let buf: u64 = 1;
        // Safe as we are reading
        let ret = unsafe {
            libc::write(
                self.eventfd.as_raw_fd(),
                &buf as *const u64 as *const c_void,
                mem::size_of::<u64>(),
            )
        };
        if ret <= 0 {
            println!("Failed writing to irqfd {:x}", ret);
        }
        Ok(())
    }
}

impl Drop for Irqfd {
    fn drop(&mut self) {
        let mut flags = 0;

        if self.level {
            flags |= gunyah_irqfd_flags::GUNYAH_IRQFD_FLAGS_LEVEL;
        }
        self.vm
            .remove_function::<IrqfdFunction>(&gunyah_fn_irqfd_arg {
                fd: self.eventfd.as_raw_fd() as u32,
                label: self.label,
                flags,
                ..Default::default()
            })
            .unwrap();
    }
}

#[cfg(test)]
mod tests {
    use claim::{assert_err, assert_ok};

    use crate::{Gunyah, Irqfd};

    #[test]
    pub fn create_edge() {
        let gunyah = Gunyah::new().unwrap();
        let vm = gunyah.create_vm().unwrap();

        assert_ok!(Irqfd::new(vm, 0, false));
    }

    #[test]
    pub fn create_level() {
        let gunyah = Gunyah::new().unwrap();
        let vm = gunyah.create_vm().unwrap();

        assert_ok!(Irqfd::new(vm, 0, true));
    }

    #[test]
    pub fn create_many() {
        let gunyah = Gunyah::new().unwrap();
        let vm = gunyah.create_vm().unwrap();

        let mut irqfds = Vec::new();
        for label in 0..4 {
            let irqfd = Irqfd::new(vm.clone(), label, false);
            assert_ok!(&irqfd);
            irqfds.push(irqfd);
        }

        assert_err!(Irqfd::new(vm.clone(), 0, false));
        assert_err!(Irqfd::new(vm.clone(), 0, true));
    }
}
