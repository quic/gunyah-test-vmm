// Copyright (c) 2024, Qualcomm Innovation Center, Inc. All rights reserved.
// SPDX-License-Identifier: BSD-3-Clause-Clear

use std::{
    fs::File,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::prelude::RawFd,
    },
};

use anyhow::{Context, Result};
use gunyah_bindings::{gunyah_fn_ioeventfd_arg, gunyah_ioeventfd_flags};
use nix::sys::eventfd::{eventfd, EfdFlags};
use same_file::Handle;

use crate::{IoeventfdFunction, Vm};

#[derive(Debug)]
pub struct Ioeventfd {
    vm: Vm,
    addr: u64,
    len: u32,
    datamatch: Option<u64>,
    eventfd: Handle,
}

impl Ioeventfd {
    pub fn new(vm: Vm, addr: u64, len: u32, datamatch: Option<u64>) -> Result<Self> {
        let mut flags = 0;

        if datamatch.is_some() {
            flags |= gunyah_ioeventfd_flags::GUNYAH_IOEVENTFD_FLAGS_DATAMATCH;
        }

        let raw_fd = eventfd(0, EfdFlags::empty()).context("Failed to create eventfd")?;
        // SAFETY: Safe because we created the eventfd
        let eventfd = unsafe { File::from_raw_fd(raw_fd) };

        assert!(
            vm.add_function::<IoeventfdFunction>(&gunyah_fn_ioeventfd_arg {
                datamatch: datamatch.unwrap_or_default(),
                addr,
                len,
                fd: raw_fd,
                flags,
                ..Default::default()
            })
            .context("Failed to register ioeventfd with VM")?
                == 0
        );

        Ok(Self {
            vm,
            addr,
            len,
            datamatch,
            eventfd: Handle::from_file(eventfd).context("failed to stat eventfd")?,
        })
    }

    pub fn as_file(&self) -> &File {
        self.eventfd.as_file()
    }

    pub fn as_file_mut(&mut self) -> &mut File {
        self.eventfd.as_file_mut()
    }
}
impl AsRawFd for Ioeventfd {
    fn as_raw_fd(&self) -> RawFd {
        self.eventfd.as_raw_fd()
    }
}

impl PartialEq for Ioeventfd {
    fn eq(&self, other: &Self) -> bool {
        self.eventfd == other.eventfd
    }
}

impl Drop for Ioeventfd {
    fn drop(&mut self) {
        let mut flags = 0;

        if self.datamatch.is_some() {
            flags |= gunyah_ioeventfd_flags::GUNYAH_IOEVENTFD_FLAGS_DATAMATCH;
        }

        self.vm
            .remove_function::<IoeventfdFunction>(&gunyah_fn_ioeventfd_arg {
                datamatch: self.datamatch.unwrap_or_default(),
                addr: self.addr,
                len: self.len,
                fd: self.eventfd.as_raw_fd(),
                flags,
                ..Default::default()
            })
            .unwrap();
    }
}

#[cfg(test)]
mod tests {
    use claim::*;

    use super::*;
    use crate::Gunyah;

    #[test]
    pub fn create() {
        let gunyah = Gunyah::new().unwrap();
        let vm = gunyah.create_vm().unwrap();

        assert_ok!(Ioeventfd::new(vm, 0x8000, 4, None));
    }

    #[test]
    pub fn create_datamatch() {
        let gunyah = Gunyah::new().unwrap();
        let vm = gunyah.create_vm().unwrap();

        assert_ok!(Ioeventfd::new(vm, 0x8000, 4, Some(0x1)));
    }

    #[test]
    pub fn create_many() {
        let gunyah = Gunyah::new().unwrap();
        let vm = gunyah.create_vm().unwrap();

        let mut eventfds = Vec::new();
        for addr in &[0x8000, 0x8008, 0x800a, 0x100] {
            let eventfd = Ioeventfd::new(vm.clone(), *addr, 8, None);
            assert_ok!(&eventfd);
            eventfds.push(eventfd);
        }

        assert_err!(Ioeventfd::new(vm.clone(), 0x8000, 4, None));
        assert_err!(Ioeventfd::new(vm.clone(), 0x8000, 8, None));
        // TODO: More!
    }

    #[test]
    pub fn drops() {
        let gunyah = Gunyah::new().unwrap();
        let vm = gunyah.create_vm().unwrap();

        assert_ok!(Ioeventfd::new(vm.clone(), 0x8000, 4, None));
        assert_ok!(Ioeventfd::new(vm.clone(), 0x8000, 4, None));
    }
}
