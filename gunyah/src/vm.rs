// Copyright (c) 2024, Qualcomm Innovation Center, Inc. All rights reserved.
// SPDX-License-Identifier: BSD-3-Clause-Clear

#[cfg(feature = "ack-bindings")]
use std::collections::HashMap;
#[cfg(feature = "ack-bindings")]
use std::sync::Arc;
use std::{
    fs::File,
    mem::size_of,
    os::fd::{AsRawFd, FromRawFd, RawFd},
};

use gunyah_bindings::{
    gunyah_fn_desc, gunyah_fn_ioeventfd_arg, gunyah_fn_irqfd_arg, gunyah_fn_type,
    gunyah_fn_vcpu_arg, gunyah_map_flags, gunyah_vm_add_function, gunyah_vm_boot_context,
    gunyah_vm_boot_context_reg, gunyah_vm_boot_context_reg_id, gunyah_vm_dtb_config,
    gunyah_vm_remove_function, gunyah_vm_set_boot_context, gunyah_vm_set_dtb_config,
    gunyah_vm_start,
};
#[cfg(feature = "ack-bindings")]
use memmap::MmapMut;
use nix::unistd::dup;
use same_file::Handle;

#[cfg(feature = "ack-bindings")]
use gunyah_bindings::{
    gh_vm_android_lend_user_mem, gunyah_userspace_memory_region, gunyah_vm_set_user_mem_region,
};
#[cfg(not(feature = "ack-bindings"))]
use gunyah_bindings::{gunyah_map_mem_args, gunyah_vm_map_mem};

use crate::guest_mem::GuestMemRegion;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ShareType {
    Share,
    Lend,
}

#[derive(Clone, Copy)]
pub enum GuestMemoryAccess {
    R,
    Rw,
    Rx,
    Rwx,
}

pub trait VmFunction {
    const FUNCTION_TYPE: gunyah_fn_type::Type;
    type FunctionArg;
}

macro_rules! declare_function {
    ($n:ident, $arg:ty, $type_:expr) => {
        pub struct $n;
        impl VmFunction for $n {
            type FunctionArg = $arg;
            const FUNCTION_TYPE: gunyah_fn_type::Type = $type_;
        }
    };
}
declare_function!(
    VcpuFunction,
    gunyah_fn_vcpu_arg,
    gunyah_fn_type::GUNYAH_FN_VCPU
);

declare_function!(
    IoeventfdFunction,
    gunyah_fn_ioeventfd_arg,
    gunyah_fn_type::GUNYAH_FN_IOEVENTFD
);

declare_function!(
    IrqfdFunction,
    gunyah_fn_irqfd_arg,
    gunyah_fn_type::GUNYAH_FN_IRQFD
);

#[derive(Debug)]
pub struct Vm(
    Handle,
    #[cfg(feature = "ack-bindings")] HashMap<(u64, GuestMemRegion), Arc<MmapMut>>,
);

impl Vm {
    pub fn start(&self) -> nix::Result<()> {
        // SAFETY: Safe because we own the VM fd and know it is a gunyah-vm
        unsafe { gunyah_vm_start(self.as_raw_fd()) }.and(Ok(()))
    }

    /// add_function -- Adds a function to the VM
    ///
    /// # Example
    ///
    /// ```
    /// let vm = Gunyah::new().unwrap().create_vm().unwrap();
    /// assert_ok!(vm.add_function::<VcpuFunction>(&gunyah_fn_vcpu_arg { id: 0 }));
    /// ```
    ///
    /// ```compile_fail
    /// let vm = Gunyah::new().unwrap().create_vm().unwrap();
    /// assert_ok!(vm.add_function::<VcpuFunction>(&0));
    /// ```
    pub(crate) fn add_function<T>(&self, arg: &T::FunctionArg) -> nix::Result<i32>
    where
        T: VmFunction,
    {
        let fn_arg = gunyah_fn_desc {
            type_: T::FUNCTION_TYPE,
            arg_size: size_of::<T::FunctionArg>().try_into().unwrap(),
            arg: arg as *const T::FunctionArg as u64,
        };
        // SAFETY: Safe because we own the VM fd and we filled the arguments correctly
        unsafe { gunyah_vm_add_function(self.as_raw_fd(), &fn_arg) }
    }

    pub(crate) fn remove_function<T>(&self, arg: &T::FunctionArg) -> nix::Result<i32>
    where
        T: VmFunction,
    {
        let fn_arg = gunyah_fn_desc {
            type_: T::FUNCTION_TYPE,
            arg_size: size_of::<T::FunctionArg>().try_into().unwrap(),
            arg: arg as *const T::FunctionArg as u64,
        };
        // SAFETY: Safe because we own the VM fd and we filled the arguments correctly
        unsafe { gunyah_vm_remove_function(self.as_raw_fd(), &fn_arg) }
    }

    #[cfg(not(feature = "ack-bindings"))]
    #[allow(clippy::too_many_arguments)] // These arguments come to the ioctl. Blame the kernel.
    fn __map_memory(
        &self,
        guest_addr: u64,
        share_type: ShareType,
        access: GuestMemoryAccess,
        unmap: bool,
        region: &GuestMemRegion,
    ) -> nix::Result<()> {
        let flags = match share_type {
            ShareType::Share => gunyah_map_flags::GUNYAH_MEM_FORCE_SHARE,
            ShareType::Lend => gunyah_map_flags::GUNYAH_MEM_FORCE_LEND,
        } | match access {
            GuestMemoryAccess::R => gunyah_map_flags::GUNYAH_MEM_ALLOW_READ,
            GuestMemoryAccess::Rw => {
                gunyah_map_flags::GUNYAH_MEM_ALLOW_READ | gunyah_map_flags::GUNYAH_MEM_ALLOW_WRITE
            }
            GuestMemoryAccess::Rx => {
                gunyah_map_flags::GUNYAH_MEM_ALLOW_READ | gunyah_map_flags::GUNYAH_MEM_ALLOW_EXEC
            }
            GuestMemoryAccess::Rwx => gunyah_map_flags::GUNYAH_MEM_ALLOW_RWX,
        } | match unmap {
            true => gunyah_map_flags::GUNYAH_MEM_UNMAP,
            false => 0,
        };

        let args = gunyah_map_mem_args {
            guest_addr,
            flags,
            offset: region.offset(),
            guest_mem_fd: region.as_guest_mem().as_raw_fd() as u32,
            size: region.size() as u64,
        };

        // SAFETY: Safe because we own the VM fd and know it is a Gunyah VM fd.
        unsafe { gunyah_vm_map_mem(self.as_raw_fd(), &args) }?;
        Ok(())
    }

    #[cfg(feature = "ack-bindings")]
    pub fn __map_memory(
        &mut self,
        guest_addr: u64,
        share_type: ShareType,
        access: GuestMemoryAccess,
        unmap: bool,
        region: &GuestMemRegion,
    ) -> nix::Result<()> {
        use libc::{madvise, MADV_HUGEPAGE, MADV_NOHUGEPAGE};

        let flags = match access {
            GuestMemoryAccess::R => gunyah_map_flags::GUNYAH_MEM_ALLOW_READ,
            GuestMemoryAccess::Rw => {
                gunyah_map_flags::GUNYAH_MEM_ALLOW_READ | gunyah_map_flags::GUNYAH_MEM_ALLOW_WRITE
            }
            GuestMemoryAccess::Rx => {
                gunyah_map_flags::GUNYAH_MEM_ALLOW_READ | gunyah_map_flags::GUNYAH_MEM_ALLOW_EXEC
            }
            GuestMemoryAccess::Rwx => gunyah_map_flags::GUNYAH_MEM_ALLOW_RWX,
        } | match unmap {
            true => gunyah_map_flags::GUNYAH_MEM_UNMAP,
            false => 0,
        };

        let key = (guest_addr, region.clone());
        let userspace_addr = if unmap {
            unimplemented!();
        } else {
            let userspace_addr = Arc::new(
                // TODO: region.map() for RO access
                region.map_mut().expect("Failed to map region"),
            );
            if self.1.contains_key(&key) {
                return Err(nix::Error::EEXIST);
            }
            self.1.insert(key, userspace_addr.clone());
            userspace_addr.as_ptr()
        };

        if region.as_guest_mem().use_huge_pages() {
            unsafe {
                madvise(
                    userspace_addr as *mut libc::c_void,
                    region.size(),
                    MADV_HUGEPAGE,
                )
            };
        } else {
            unsafe {
                madvise(
                    userspace_addr as *mut libc::c_void,
                    region.size(),
                    MADV_NOHUGEPAGE,
                )
            };
        }

        let args = gunyah_userspace_memory_region {
            label: self.1.len() as u32, // so far this has been good enough to ensure labels are unique
            flags,
            userspace_addr: userspace_addr as u64,
            guest_phys_addr: guest_addr,
            memory_size: region.size() as u64,
        };

        println!("{:?}", args);

        match share_type {
            ShareType::Share => {
                // SAFETY: Safe because we own the VM fd and know it is a Gunyah VM fd.
                unsafe { gunyah_vm_set_user_mem_region(self.as_raw_fd(), &args) }?;
            }
            ShareType::Lend => {
                // SAFETY: Safe because we own the VM fd and know it is a Gunyah VM fd.
                unsafe { gh_vm_android_lend_user_mem(self.as_raw_fd(), &args) }?;
            }
        };
        Ok(())
    }

    pub fn map_memory(
        &mut self,
        guest_addr: u64,
        share_type: ShareType,
        access: GuestMemoryAccess,
        region: &GuestMemRegion,
    ) -> nix::Result<()> {
        self.__map_memory(guest_addr, share_type, access, false, region)
    }

    pub fn unmap_memory(
        &mut self,
        guest_addr: u64,
        share_type: ShareType,
        access: GuestMemoryAccess,
        region: &GuestMemRegion,
    ) -> nix::Result<()> {
        self.__map_memory(guest_addr, share_type, access, true, region)
    }

    pub fn dup(&self) -> nix::Result<Self> {
        // SAFETY: Safe because fd our fd is a GuestMem and the resulting dup'd
        // fd is also a GuestMem
        let file = unsafe { File::from_raw_fd(dup(self.as_raw_fd())?) };
        Ok(Self(
            Handle::from_file(file).map_err(|e| {
                e.raw_os_error()
                    .map_or(nix::Error::UnknownErrno, nix::Error::from_i32)
            })?,
            #[cfg(feature = "ack-bindings")]
            self.1.clone(),
        ))
    }

    pub fn as_file(&self) -> &File {
        self.0.as_file()
    }

    pub fn as_file_mut(&mut self) -> &mut File {
        self.0.as_file_mut()
    }

    pub fn into_file(self) -> File {
        // SAFETY: Safe because fd our fd is a Vm and the resulting dup'd
        // fd is also a Vm
        unsafe { File::from_raw_fd(dup(self.as_raw_fd()).expect("Unable to dup vm descriptor")) }
    }

    pub fn set_dtb_config(&self, guest_phys_addr: u64, size: u64) -> nix::Result<()> {
        // SAFETY: Safe because we know fd is a gunyah-vm and
        // gunyah_vm_set_dtb_config is a valid ioctl on gunyah-vm fds
        unsafe {
            gunyah_vm_set_dtb_config(
                self.as_raw_fd(),
                &gunyah_vm_dtb_config {
                    guest_phys_addr,
                    size,
                },
            )
        }
        .and(Ok(()))
    }

    fn set_boot_context(
        &self,
        reg_type: gunyah_vm_boot_context_reg::Type,
        reg_idx: u8,
        value: u64,
    ) -> nix::Result<()> {
        unsafe {
            gunyah_vm_set_boot_context(
                self.as_raw_fd(),
                &gunyah_vm_boot_context {
                    reg: gunyah_vm_boot_context_reg_id(reg_type, reg_idx),
                    value,
                    ..Default::default()
                },
            )
        }
        .and(Ok(()))
    }

    pub fn set_boot_pc(&self, value: u64) -> nix::Result<()> {
        self.set_boot_context(gunyah_vm_boot_context_reg::REG_SET_PC, 0, value)
    }

    pub fn set_boot_sp(&self, value: u64) -> nix::Result<()> {
        self.set_boot_context(gunyah_vm_boot_context_reg::REG_SET_SP, 1, value)
    }
}

impl AsRawFd for Vm {
    fn as_raw_fd(&self) -> RawFd {
        self.0.as_raw_fd()
    }
}

impl From<File> for Vm {
    fn from(file: File) -> Self {
        Self(
            Handle::from_file(file).expect("Unable to get info about file"),
            #[cfg(feature = "ack-bindings")]
            Default::default(),
        )
    }
}

impl Clone for Vm {
    fn clone(&self) -> Self {
        self.dup()
            .unwrap_or_else(|_| panic!("Failed to dup {:?}", self.0))
    }
}

#[cfg(all(test, not(feature = "ack-bindings")))]
mod tests {
    use std::num::NonZeroUsize;

    use claim::*;
    use gunyah_bindings::gunyah_vm_boot_context_reg::{REG_SET_PC, REG_SET_X};

    use super::*;
    use crate::Gunyah;

    macro_rules! mib {
        ($x:expr) => {
            $x * 1048576
        };
    }

    #[test]
    fn map_memory() {
        let gunyah = Gunyah::new().unwrap();
        let mem = gunyah
            .create_guest_memory(NonZeroUsize::new(mib!(10)).unwrap(), false)
            .unwrap();
        let mut vm = gunyah.create_vm().unwrap();

        assert_ok!(vm.map_memory(
            0x8000_0000,
            ShareType::Share,
            GuestMemoryAccess::Rwx,
            &GuestMemRegion::new(mem, 0, NonZeroUsize::new(mib!(10)).unwrap()).unwrap()
        ));
    }

    #[test]
    fn unmap_memory_single() {
        let gunyah = Gunyah::new().unwrap();
        let mem = gunyah
            .create_guest_memory(NonZeroUsize::new(mib!(10)).unwrap(), false)
            .unwrap();
        let mut vm = gunyah.create_vm().unwrap();

        assert_ok!(vm.map_memory(
            0x8000_0000,
            ShareType::Share,
            GuestMemoryAccess::Rwx,
            &GuestMemRegion::new(mem.clone(), 0, NonZeroUsize::new(mib!(10)).unwrap()).unwrap()
        ));
        assert_err!(vm.unmap_memory(
            0x7000_0000,
            ShareType::Share,
            GuestMemoryAccess::Rwx,
            &GuestMemRegion::new(mem.clone(), 0, NonZeroUsize::new(mib!(10)).unwrap()).unwrap()
        ));
        assert_err!(vm.unmap_memory(
            0x8000_0000,
            ShareType::Lend,
            GuestMemoryAccess::Rwx,
            &GuestMemRegion::new(mem.clone(), 0, NonZeroUsize::new(mib!(10)).unwrap()).unwrap()
        ));
        assert_err!(vm.unmap_memory(
            0x8000_0000,
            ShareType::Share,
            GuestMemoryAccess::Rx,
            &GuestMemRegion::new(mem.clone(), 0, NonZeroUsize::new(mib!(10)).unwrap()).unwrap()
        ));
        assert_err!(vm.unmap_memory(
            0x8000_0000,
            ShareType::Share,
            GuestMemoryAccess::Rwx,
            &GuestMemRegion::new(mem.clone(), mib!(1), NonZeroUsize::new(mib!(10)).unwrap())
                .unwrap()
        ));
        assert_ok!(vm.unmap_memory(
            0x8000_0000,
            ShareType::Share,
            GuestMemoryAccess::Rwx,
            &GuestMemRegion::new(mem.clone(), 0, NonZeroUsize::new(mib!(10)).unwrap()).unwrap()
        ));
    }

    #[test]
    fn unmap_memory_single2() {
        let gunyah = Gunyah::new().unwrap();
        let mem = gunyah
            .create_guest_memory(NonZeroUsize::new(mib!(10)).unwrap(), false)
            .unwrap();
        let mut vm = gunyah.create_vm().unwrap();

        assert_ok!(vm.map_memory(
            0x8000_0000,
            ShareType::Share,
            GuestMemoryAccess::Rwx,
            &GuestMemRegion::new(mem.clone(), 0, NonZeroUsize::new(mib!(10)).unwrap()).unwrap()
        ));
        assert_ok!(vm.unmap_memory(
            0x8000_0000 + mib!(1),
            ShareType::Share,
            GuestMemoryAccess::Rwx,
            &GuestMemRegion::new(mem.clone(), mib!(1), NonZeroUsize::new(mib!(9)).unwrap())
                .unwrap()
        ));
        assert_ok!(vm.unmap_memory(
            0x8000_0000,
            ShareType::Share,
            GuestMemoryAccess::Rwx,
            &GuestMemRegion::new(mem.clone(), 0, NonZeroUsize::new(mib!(1)).unwrap()).unwrap()
        ));
    }

    #[test]
    fn unmap_memory_single3() {
        let gunyah = Gunyah::new().unwrap();
        let mem = gunyah
            .create_guest_memory(NonZeroUsize::new(mib!(10)).unwrap(), false)
            .unwrap();
        let mut vm = gunyah.create_vm().unwrap();

        assert_ok!(vm.map_memory(
            0x8000_0000,
            ShareType::Share,
            GuestMemoryAccess::Rwx,
            &GuestMemRegion::new(mem.clone(), 0, NonZeroUsize::new(mib!(10)).unwrap()).unwrap()
        ));
        assert_ok!(vm.unmap_memory(
            0x8000_0000,
            ShareType::Share,
            GuestMemoryAccess::Rwx,
            &GuestMemRegion::new(mem.clone(), 0, NonZeroUsize::new(mib!(1)).unwrap()).unwrap()
        ));
        assert_ok!(vm.unmap_memory(
            0x8000_0000 + mib!(1),
            ShareType::Share,
            GuestMemoryAccess::Rwx,
            &GuestMemRegion::new(mem.clone(), mib!(1), NonZeroUsize::new(mib!(9)).unwrap())
                .unwrap()
        ));
    }

    #[test]
    fn unmap_memory_single4() {
        let gunyah = Gunyah::new().unwrap();
        let mem = gunyah
            .create_guest_memory(NonZeroUsize::new(mib!(10)).unwrap(), false)
            .unwrap();
        let mut vm = gunyah.create_vm().unwrap();

        assert_ok!(vm.map_memory(
            0x8000_0000,
            ShareType::Share,
            GuestMemoryAccess::Rwx,
            &GuestMemRegion::new(mem.clone(), mib!(1), NonZeroUsize::new(mib!(5)).unwrap())
                .unwrap()
        ));
        assert_ok!(vm.unmap_memory(
            0x8000_0000,
            ShareType::Share,
            GuestMemoryAccess::Rwx,
            &GuestMemRegion::new(mem.clone(), mib!(1), NonZeroUsize::new(mib!(1)).unwrap())
                .unwrap()
        ));
        assert_ok!(vm.unmap_memory(
            0x8000_0000 + mib!(1),
            ShareType::Share,
            GuestMemoryAccess::Rwx,
            &GuestMemRegion::new(mem.clone(), mib!(2), NonZeroUsize::new(mib!(4)).unwrap())
                .unwrap()
        ));
    }

    #[test]
    fn unmap_memory_multiple_simple1() {
        let gunyah = Gunyah::new().unwrap();
        let mem = gunyah
            .create_guest_memory(NonZeroUsize::new(mib!(20)).unwrap(), false)
            .unwrap();
        let mut vm = gunyah.create_vm().unwrap();

        // Simple
        assert_ok!(vm.map_memory(
            0x8000_0000,
            ShareType::Share,
            GuestMemoryAccess::Rwx,
            &GuestMemRegion::new(mem.clone(), 0, NonZeroUsize::new(mib!(10)).unwrap()).unwrap()
        ));
        assert_ok!(vm.map_memory(
            0x8000_0000 + mib!(10),
            ShareType::Share,
            GuestMemoryAccess::Rwx,
            &GuestMemRegion::new(mem.clone(), mib!(10), NonZeroUsize::new(mib!(10)).unwrap())
                .unwrap()
        ));
        assert_ok!(vm.unmap_memory(
            0x8000_0000,
            ShareType::Share,
            GuestMemoryAccess::Rwx,
            &GuestMemRegion::new(mem.clone(), 0, NonZeroUsize::new(mib!(10)).unwrap()).unwrap()
        ));
        assert_ok!(vm.unmap_memory(
            0x8000_0000 + mib!(10),
            ShareType::Share,
            GuestMemoryAccess::Rwx,
            &GuestMemRegion::new(mem.clone(), mib!(10), NonZeroUsize::new(mib!(10)).unwrap())
                .unwrap()
        ));
    }

    #[test]
    fn unmap_memory_multiple_simple2() {
        let gunyah = Gunyah::new().unwrap();
        let mem = gunyah
            .create_guest_memory(NonZeroUsize::new(mib!(20)).unwrap(), false)
            .unwrap();
        let mut vm = gunyah.create_vm().unwrap();

        // Same, but unmap the other one first
        assert_ok!(vm.map_memory(
            0x8000_0000,
            ShareType::Share,
            GuestMemoryAccess::Rwx,
            &GuestMemRegion::new(mem.clone(), 0, NonZeroUsize::new(mib!(10)).unwrap()).unwrap()
        ));
        assert_ok!(vm.map_memory(
            0x8000_0000 + mib!(10),
            ShareType::Share,
            GuestMemoryAccess::Rwx,
            &GuestMemRegion::new(mem.clone(), mib!(10), NonZeroUsize::new(mib!(10)).unwrap())
                .unwrap()
        ));
        assert_ok!(vm.unmap_memory(
            0x8000_0000 + mib!(10),
            ShareType::Share,
            GuestMemoryAccess::Rwx,
            &GuestMemRegion::new(mem.clone(), mib!(10), NonZeroUsize::new(mib!(10)).unwrap())
                .unwrap()
        ));
        assert_ok!(vm.unmap_memory(
            0x8000_0000,
            ShareType::Share,
            GuestMemoryAccess::Rwx,
            &GuestMemRegion::new(mem.clone(), 0, NonZeroUsize::new(mib!(10)).unwrap()).unwrap()
        ));
    }

    #[test]
    fn unmap_memory_multiple_simple3() {
        let gunyah = Gunyah::new().unwrap();
        let mem = gunyah
            .create_guest_memory(NonZeroUsize::new(mib!(30)).unwrap(), false)
            .unwrap();
        let mut vm = gunyah.create_vm().unwrap();

        // Simple with 3
        assert_ok!(vm.map_memory(
            0x8000_0000,
            ShareType::Share,
            GuestMemoryAccess::Rwx,
            &GuestMemRegion::new(mem.clone(), 0, NonZeroUsize::new(mib!(10)).unwrap()).unwrap()
        ));
        assert_ok!(vm.map_memory(
            0x8000_0000 + mib!(10),
            ShareType::Share,
            GuestMemoryAccess::Rwx,
            &GuestMemRegion::new(mem.clone(), mib!(10), NonZeroUsize::new(mib!(10)).unwrap())
                .unwrap()
        ));
        assert_ok!(vm.map_memory(
            0x8000_0000 + mib!(20),
            ShareType::Share,
            GuestMemoryAccess::Rwx,
            &GuestMemRegion::new(mem.clone(), mib!(20), NonZeroUsize::new(mib!(10)).unwrap())
                .unwrap()
        ));
        assert_ok!(vm.unmap_memory(
            0x8000_0000 + mib!(10),
            ShareType::Share,
            GuestMemoryAccess::Rwx,
            &GuestMemRegion::new(mem.clone(), mib!(10), NonZeroUsize::new(mib!(10)).unwrap())
                .unwrap()
        ));
        assert_ok!(vm.unmap_memory(
            0x8000_0000,
            ShareType::Share,
            GuestMemoryAccess::Rwx,
            &GuestMemRegion::new(mem.clone(), 0, NonZeroUsize::new(mib!(10)).unwrap()).unwrap()
        ));
        assert_ok!(vm.unmap_memory(
            0x8000_0000 + mib!(20),
            ShareType::Share,
            GuestMemoryAccess::Rwx,
            &GuestMemRegion::new(mem.clone(), mib!(20), NonZeroUsize::new(mib!(10)).unwrap())
                .unwrap()
        ));
    }

    #[test]
    #[ignore = "todo on kernel side to better support partial unmapping"]
    fn unmap_memory_multiple_partial_unmap() {
        let gunyah = Gunyah::new().unwrap();
        let mem = gunyah
            .create_guest_memory(NonZeroUsize::new(mib!(20)).unwrap(), false)
            .unwrap();
        let mut vm = gunyah.create_vm().unwrap();

        // Partial unmapping
        assert_ok!(vm.map_memory(
            0x8000_0000,
            ShareType::Share,
            GuestMemoryAccess::Rwx,
            &GuestMemRegion::new(mem.clone(), 0, NonZeroUsize::new(mib!(10)).unwrap()).unwrap()
        ));
        assert_ok!(vm.map_memory(
            0x8000_0000 + mib!(10),
            ShareType::Share,
            GuestMemoryAccess::Rwx,
            &GuestMemRegion::new(mem.clone(), 0, NonZeroUsize::new(mib!(10)).unwrap()).unwrap()
        ));
        assert_ok!(vm.unmap_memory(
            0x8000_0000 + mib!(10),
            ShareType::Share,
            GuestMemoryAccess::Rwx,
            &GuestMemRegion::new(mem.clone(), 0, NonZeroUsize::new(mib!(10)).unwrap()).unwrap()
        ));
        assert_ok!(vm.unmap_memory(
            0x8000_0000,
            ShareType::Share,
            GuestMemoryAccess::Rwx,
            &GuestMemRegion::new(mem.clone(), 0, NonZeroUsize::new(mib!(1)).unwrap()).unwrap()
        ));
        assert_ok!(vm.unmap_memory(
            0x8000_0000 + mib!(1),
            ShareType::Share,
            GuestMemoryAccess::Rwx,
            &GuestMemRegion::new(mem.clone(), mib!(1), NonZeroUsize::new(mib!(9)).unwrap())
                .unwrap()
        ));

        assert_ok!(vm.map_memory(
            0x8000_0000,
            ShareType::Share,
            GuestMemoryAccess::Rwx,
            &GuestMemRegion::new(mem.clone(), 0, NonZeroUsize::new(mib!(10)).unwrap()).unwrap()
        ));
        assert_ok!(vm.map_memory(
            0x8000_0000 + mib!(10),
            ShareType::Share,
            GuestMemoryAccess::Rwx,
            &GuestMemRegion::new(mem.clone(), 0, NonZeroUsize::new(mib!(10)).unwrap()).unwrap()
        ));
        assert_ok!(vm.unmap_memory(
            0x8000_0000,
            ShareType::Share,
            GuestMemoryAccess::Rwx,
            &GuestMemRegion::new(mem.clone(), 0, NonZeroUsize::new(mib!(1)).unwrap()).unwrap()
        ));
        assert_ok!(vm.unmap_memory(
            0x8000_0000 + mib!(1),
            ShareType::Share,
            GuestMemoryAccess::Rwx,
            &GuestMemRegion::new(mem.clone(), mib!(1), NonZeroUsize::new(mib!(9)).unwrap())
                .unwrap()
        ));
        assert_ok!(vm.unmap_memory(
            0x8000_0000 + mib!(10),
            ShareType::Share,
            GuestMemoryAccess::Rwx,
            &GuestMemRegion::new(mem.clone(), 0, NonZeroUsize::new(mib!(10)).unwrap()).unwrap()
        ));

        assert_ok!(vm.map_memory(
            0x8000_0000,
            ShareType::Share,
            GuestMemoryAccess::Rwx,
            &GuestMemRegion::new(mem.clone(), 0, NonZeroUsize::new(mib!(10)).unwrap()).unwrap()
        ));
        assert_ok!(vm.map_memory(
            0x8000_0000 + mib!(10),
            ShareType::Share,
            GuestMemoryAccess::Rwx,
            &GuestMemRegion::new(mem.clone(), 0, NonZeroUsize::new(mib!(10)).unwrap()).unwrap()
        ));
        assert_ok!(vm.unmap_memory(
            0x8000_0000,
            ShareType::Share,
            GuestMemoryAccess::Rwx,
            &GuestMemRegion::new(mem.clone(), 0, NonZeroUsize::new(mib!(1)).unwrap()).unwrap()
        ));
        assert_ok!(vm.unmap_memory(
            0x8000_0000 + mib!(10),
            ShareType::Share,
            GuestMemoryAccess::Rwx,
            &GuestMemRegion::new(mem.clone(), 0, NonZeroUsize::new(mib!(10)).unwrap()).unwrap()
        ));
        assert_ok!(vm.unmap_memory(
            0x8000_0000 + mib!(1),
            ShareType::Share,
            GuestMemoryAccess::Rwx,
            &GuestMemRegion::new(mem.clone(), mib!(1), NonZeroUsize::new(mib!(9)).unwrap())
                .unwrap()
        ));
    }

    #[test]
    fn map_memory_ioff() {
        let gunyah = Gunyah::new().unwrap();
        let mem = gunyah
            .create_guest_memory(NonZeroUsize::new(mib!(20)).unwrap(), false)
            .unwrap();
        let mut vm = gunyah.create_vm().unwrap();
        let mut vm2 = gunyah.create_vm().unwrap();

        assert_ok!(vm.map_memory(
            0x8000_0000,
            ShareType::Share,
            GuestMemoryAccess::Rwx,
            &GuestMemRegion::new(mem.clone(), 0, NonZeroUsize::new(mib!(10)).unwrap()).unwrap()
        ));

        // Same, but with +10MB guest offset
        assert_err!(vm.map_memory(
            0x8000_0000 + mib!(10),
            ShareType::Share,
            GuestMemoryAccess::Rwx,
            &GuestMemRegion::new(mem.clone(), 0, NonZeroUsize::new(mib!(10)).unwrap()).unwrap()
        ));

        // Same, but with lend
        assert_err!(vm.map_memory(
            0x8000_0000,
            ShareType::Lend,
            GuestMemoryAccess::Rwx,
            &GuestMemRegion::new(mem.clone(), 0, NonZeroUsize::new(mib!(10)).unwrap()).unwrap()
        ));

        // Same, but with lend and +10MB
        assert_err!(vm.map_memory(
            0x8000_0000 + mib!(10),
            ShareType::Lend,
            GuestMemoryAccess::Rwx,
            &GuestMemRegion::new(mem.clone(), 0, NonZeroUsize::new(mib!(10)).unwrap()).unwrap()
        ));

        // Same, but with lend and +10MB and different vm
        assert_err!(vm2.map_memory(
            0x8000_0000 + mib!(10),
            ShareType::Lend,
            GuestMemoryAccess::Rwx,
            &GuestMemRegion::new(mem.clone(), 0, NonZeroUsize::new(mib!(10)).unwrap()).unwrap()
        ));

        assert_ok!(vm.map_memory(
            0x8000_0000 + mib!(10),
            ShareType::Share,
            GuestMemoryAccess::Rwx,
            &GuestMemRegion::new(mem.clone(), mib!(10), NonZeroUsize::new(mib!(10)).unwrap())
                .unwrap()
        ));
    }

    #[test]
    fn add_vcpu() {
        let gunyah = Gunyah::new().unwrap();
        let vm = gunyah.create_vm().unwrap();

        assert_ok!(vm.add_function::<VcpuFunction>(&gunyah_fn_vcpu_arg { id: 0 }));
        assert_err!(vm.add_function::<VcpuFunction>(&gunyah_fn_vcpu_arg { id: 0 }));

        assert_ok!(vm.add_function::<VcpuFunction>(&gunyah_fn_vcpu_arg { id: 1 }));

        assert_ok!(vm.add_function::<VcpuFunction>(&gunyah_fn_vcpu_arg { id: 2 }));

        assert_ok!(vm.remove_function::<VcpuFunction>(&gunyah_fn_vcpu_arg { id: 0 }));
        assert_err!(vm.remove_function::<VcpuFunction>(&gunyah_fn_vcpu_arg { id: 0 }));
        assert_err!(vm.remove_function::<VcpuFunction>(&gunyah_fn_vcpu_arg { id: 500 }));

        assert_ok!(vm.remove_function::<VcpuFunction>(&gunyah_fn_vcpu_arg { id: 1 }));

        assert_ok!(vm.remove_function::<VcpuFunction>(&gunyah_fn_vcpu_arg { id: 2 }));
    }

    #[test]
    fn set_dtb_config() {
        let gunyah = Gunyah::new().unwrap();
        let vm = gunyah.create_vm().unwrap();

        assert_ok!(vm.set_dtb_config(0, mib!(1)));
    }

    #[test]
    fn set_boot_context() {
        let gunyah = Gunyah::new().unwrap();
        let vm = gunyah.create_vm().unwrap();

        for i in 0..32 {
            assert_ok!(vm.set_boot_context(REG_SET_X, i, 0xd00d));
        }
        assert_ok!(vm.set_boot_context(REG_SET_X, 0, u64::MAX));
        assert_err!(vm.set_boot_context(REG_SET_X, 32, 0xd00d));
        assert_ok!(vm.set_boot_context(REG_SET_PC, 0, 0x8000_0000));
        assert_err!(vm.set_boot_context(REG_SET_PC, 1, 0x8000_0000));
    }
}
