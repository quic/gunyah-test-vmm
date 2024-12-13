// Copyright (c) 2024, Qualcomm Innovation Center, Inc. All rights reserved.
// SPDX-License-Identifier: BSD-3-Clause-Clear

#[cfg(feature = "ack-bindings")]
use crate::ack_bindings::*;
#[cfg(not(feature = "ack-bindings"))]
use crate::bindings::*;

ioctl_write_int_bad!(gunyah_create_vm, request_code_none!(GUNYAH_IOCTL_TYPE, 0));
ioctl_write_ptr!(
    gunyah_vm_set_dtb_config,
    GUNYAH_IOCTL_TYPE,
    2,
    gunyah_vm_dtb_config
);
ioctl_none!(gunyah_vm_start, GUNYAH_IOCTL_TYPE, 3);
ioctl_write_ptr!(gunyah_vm_add_function, GUNYAH_IOCTL_TYPE, 4, gunyah_fn_desc);
ioctl_none!(gunyah_vcpu_run, GUNYAH_IOCTL_TYPE, 5);
ioctl_none!(gunyah_vcpu_mmap_size, GUNYAH_IOCTL_TYPE, 6);
ioctl_write_ptr!(
    gunyah_vm_remove_function,
    GUNYAH_IOCTL_TYPE,
    7,
    gunyah_fn_desc
);

#[cfg(not(feature = "ack-bindings"))]
ioctl_write_ptr!(
    gunyah_create_guest_mem,
    GUNYAH_IOCTL_TYPE,
    8,
    gunyah_create_mem_args
);
#[cfg(not(feature = "ack-bindings"))]
ioctl_write_ptr!(gunyah_vm_map_mem, GUNYAH_IOCTL_TYPE, 9, gunyah_map_mem_args);

#[cfg(feature = "ack-bindings")]
ioctl_write_ptr!(
    gunyah_vm_set_user_mem_region,
    GUNYAH_IOCTL_TYPE,
    0x1,
    gunyah_userspace_memory_region
);
#[cfg(feature = "ack-bindings")]
ioctl_write_ptr!(
    gh_vm_android_lend_user_mem,
    GH_ANDROID_IOCTL_TYPE,
    0x11,
    gunyah_userspace_memory_region
);
#[cfg(feature = "ack-bindings")]
ioctl_write_ptr!(
    gh_vm_android_set_fw_config,
    GH_ANDROID_IOCTL_TYPE,
    0x12,
    gunyah_vm_firmware_config
);

ioctl_write_ptr!(
    gunyah_vm_set_boot_context,
    GUNYAH_IOCTL_TYPE,
    0xa,
    gunyah_vm_boot_context
);

pub const fn gunyah_vm_boot_context_reg_id(
    reg_type: gunyah_vm_boot_context_reg::Type,
    reg_idx: u8,
) -> u32 {
    ((reg_type & 0xff) << GUNYAH_VM_BOOT_CONTEXT_REG_SHIFT) | (reg_idx as u32)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::undocumented_unsafe_blocks)]

    use std::mem::size_of;

    #[cfg(not(feature = "ack-bindings"))]
    use byte_unit::n_mib_bytes;
    use claim::*;
    use libc::{c_char, open, O_RDWR};
    use nix::sys::eventfd::{eventfd, EfdFlags};

    use crate::gunyah_vm_boot_context_reg::REG_SET_X;

    use super::*;
    const GUNYAH_PATH: &str = "/dev/gunyah\0";

    fn gunyah() -> i32 {
        let sys_fd = unsafe { open(GUNYAH_PATH.as_ptr() as *const c_char, O_RDWR) };
        assert_ge!(sys_fd, 0, "Failed to open gunyah {:?}", nix::Error::last());
        sys_fd
    }

    #[test]
    fn create_vm_fd() {
        let vm_fd = unsafe { gunyah_create_vm(gunyah(), 0) };
        assert_ok!(vm_fd);
        assert_ge!(vm_fd.unwrap(), 0);
    }

    #[cfg(not(feature = "ack-bindings"))]
    #[test]
    fn create_mem_fd() {
        let args = gunyah_create_mem_args {
            size: n_mib_bytes(1),
            ..Default::default()
        };
        let mem_fd = unsafe { gunyah_create_guest_mem(gunyah(), &args) };
        assert_ok!(mem_fd);
        assert_ge!(mem_fd.unwrap(), 0);
    }

    #[cfg(not(feature = "ack-bindings"))]
    #[test]
    fn create_mem_fd_invalid_flags() {
        let mem_args = gunyah_create_mem_args {
            flags: 0b1000, // Set to something we know is currently invalid, will need to update as more flags are used
            ..Default::default()
        };
        let mem_fd = unsafe { gunyah_create_guest_mem(gunyah(), &mem_args) };
        assert_err!(mem_fd);
    }

    #[test]
    fn set_dtb_config() {
        let vm_fd = unsafe { gunyah_create_vm(gunyah(), 0) };
        assert_ok!(vm_fd);
        assert_ge!(vm_fd.unwrap(), 0);

        assert_ok!(unsafe {
            gunyah_vm_set_dtb_config(
                vm_fd.unwrap(),
                &gunyah_vm_dtb_config {
                    guest_phys_addr: 0,
                    size: 4096,
                },
            )
        });
    }

    // No basic test for VM_START here b/c there's too much set up

    #[test]
    fn add_vcpu() {
        let vm_fd = unsafe { gunyah_create_vm(gunyah(), 0) };
        assert_ok!(vm_fd);
        assert_ge!(vm_fd.unwrap(), 0);

        let fn_arg = gunyah_fn_desc {
            type_: gunyah_fn_type::GUNYAH_FN_VCPU,
            arg_size: size_of::<gunyah_fn_vcpu_arg>().try_into().unwrap(),
            arg: &gunyah_fn_vcpu_arg { id: 0 } as *const gunyah_fn_vcpu_arg as u64,
        };
        let result = unsafe { gunyah_vm_add_function(vm_fd.unwrap(), &fn_arg) };
        assert_ok!(result);
        assert_ge!(result.unwrap(), 0);

        assert_ok!(unsafe { gunyah_vm_remove_function(vm_fd.unwrap(), &fn_arg) });
    }

    // No basic test for VCPU_RUN here b/c there's too much set up

    #[test]
    fn vcpu_mmap() {
        let vm_fd = unsafe { gunyah_create_vm(gunyah(), 0) };
        assert_ok!(vm_fd);
        assert_ge!(vm_fd.unwrap(), 0);

        let fn_arg = gunyah_fn_desc {
            type_: gunyah_fn_type::GUNYAH_FN_VCPU,
            arg_size: size_of::<gunyah_fn_vcpu_arg>().try_into().unwrap(),
            arg: &gunyah_fn_vcpu_arg { id: 0 } as *const gunyah_fn_vcpu_arg as u64,
        };
        let result = unsafe { gunyah_vm_add_function(vm_fd.unwrap(), &fn_arg) };
        assert_ok!(result);
        assert_ge!(result.unwrap(), 0);

        let mmap_size = unsafe { gunyah_vcpu_mmap_size(result.unwrap()) };
        assert_ok!(mmap_size);
        assert_ge!(
            mmap_size.unwrap(),
            size_of::<gunyah_vcpu_run>().try_into().unwrap()
        );
    }

    #[test]
    fn irqfd() {
        let vm_fd = unsafe { gunyah_create_vm(gunyah(), 0) };
        assert_ok!(vm_fd);
        assert_ge!(vm_fd.unwrap(), 0);

        let eventfd = eventfd(0, EfdFlags::empty()).expect("Failed to create eventfd");

        let fn_arg = gunyah_fn_desc {
            type_: gunyah_fn_type::GUNYAH_FN_IRQFD,
            arg_size: size_of::<gunyah_fn_irqfd_arg>().try_into().unwrap(),
            arg: &gunyah_fn_irqfd_arg {
                fd: eventfd as u32,
                label: 0,
                flags: 0,
                ..Default::default()
            } as *const gunyah_fn_irqfd_arg as u64,
        };
        let result = unsafe { gunyah_vm_add_function(vm_fd.unwrap(), &fn_arg) };
        assert_ok!(result);
        assert_eq!(result.unwrap(), 0);

        assert_ok!(unsafe { gunyah_vm_remove_function(vm_fd.unwrap(), &fn_arg) });
    }

    #[test]
    fn ioeventfd() {
        let vm_fd = unsafe { gunyah_create_vm(gunyah(), 0) };
        assert_ok!(vm_fd);
        assert_ge!(vm_fd.unwrap(), 0);

        let eventfd = eventfd(0, EfdFlags::empty()).expect("Failed to create eventfd");

        let fn_arg = gunyah_fn_desc {
            type_: gunyah_fn_type::GUNYAH_FN_IOEVENTFD,
            arg_size: size_of::<gunyah_fn_ioeventfd_arg>().try_into().unwrap(),
            arg: &gunyah_fn_ioeventfd_arg {
                fd: eventfd,
                addr: 0xdead0000,
                len: 8,
                flags: 0,
                ..Default::default()
            } as *const gunyah_fn_ioeventfd_arg as u64,
        };
        let result = unsafe { gunyah_vm_add_function(vm_fd.unwrap(), &fn_arg) };
        assert_ok!(result);
        assert_eq!(result.unwrap(), 0);

        assert_ok!(unsafe { gunyah_vm_remove_function(vm_fd.unwrap(), &fn_arg) });
    }

    #[cfg(not(feature = "ack-bindings"))]
    #[test]
    fn map_mem_fd() {
        let mem_args = gunyah_create_mem_args {
            size: n_mib_bytes!(100),
            ..Default::default()
        };
        let mem_fd = unsafe { gunyah_create_guest_mem(gunyah(), &mem_args) };
        assert_ok!(mem_fd);

        let vm_fd = unsafe { gunyah_create_vm(gunyah(), 0) };
        assert_ok!(vm_fd);

        let map_args = gunyah_map_mem_args {
            guest_addr: 0x4000,
            flags: gunyah_map_flags::GUNYAH_MEM_DEFAULT_ACCESS
                | gunyah_map_flags::GUNYAH_MEM_ALLOW_RWX,
            guest_mem_fd: mem_fd.unwrap().try_into().unwrap(),
            offset: 0,
            size: n_mib_bytes!(4),
        };
        assert_ok!(unsafe { gunyah_vm_map_mem(vm_fd.unwrap(), &map_args) });
    }

    #[test]
    fn set_boot_context() {
        let vm_fd = unsafe { gunyah_create_vm(gunyah(), 0) };
        assert_ok!(vm_fd);
        assert_ge!(vm_fd.unwrap(), 0);

        let boot_ctx = gunyah_vm_boot_context {
            reg: gunyah_vm_boot_context_reg_id(REG_SET_X, 0),
            value: 0xd00d,
            ..Default::default()
        };

        let result = unsafe { gunyah_vm_set_boot_context(vm_fd.unwrap(), &boot_ctx) };
        assert_ok!(result);
    }
}
