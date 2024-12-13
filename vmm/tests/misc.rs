// Copyright (c) 2024, Qualcomm Innovation Center, Inc. All rights reserved.
// SPDX-License-Identifier: BSD-3-Clause-Clear

use anyhow::Result;
use claim::{assert_err, assert_ok};
use gunyah::{GuestMemoryAccess, ShareType};
use vm_fdt::FdtWriter;
use vmm::GunyahVirtualMachine;

macro_rules! kib {
    ($x:expr) => {
        $x * 1024
    };
}

fn generate_fdt(vm: &GunyahVirtualMachine) -> Result<Vec<u8>> {
    let mut fdt = FdtWriter::new()?;
    let root_node = fdt.begin_node("")?;
    vm.create_fdt_basic_config(
        &mut fdt,
        &[0x3FFF0000, 0x10000, 0x3FF00000, 0x20000],
        &[13, 14, 11, 10],
    )?;
    fdt.end_node(root_node)?;
    Ok(fdt.finish()?)
}

/// Ensures that VM DTB can't be mapped when launching the VM. This essentially
/// tests that kernel ensures pages aren't mapped when assembling the RM parcel
/// Not applicable to GUP
#[test]
#[cfg(not(feature = "ack-bindings"))]
fn vm_dtb_no_map() {
    let mut vm = GunyahVirtualMachine::new().expect("Failed to create Gunyah Virtual machine");
    let mem = vm
        .add_memory(
            0x8000_0000,
            kib!(16).try_into().unwrap(),
            ShareType::Lend,
            GuestMemoryAccess::Rwx,
            false,
        )
        .expect("Failed to create guest memory");

    vm.create_vcpu(0).expect("Failed to create vcpu");

    let dtb = generate_fdt(&vm).expect("Failed to generate DT");
    vm.set_dtb_config(0x8000_0000, kib!(4), &dtb)
        .expect("Failed to set DTB configuration");

    let map = mem
        .lock()
        .unwrap()
        .as_region()
        .map()
        .expect("Failed to mmap region");
    assert_err!(vm.start());
    drop(map);
}

#[test]
fn shared_vm() {
    let mut vm = GunyahVirtualMachine::new().expect("Failed to create Gunyah Virtual machine");
    vm.add_regular_memory(
        0x8000_0000,
        kib!(16).try_into().unwrap(),
        ShareType::Share,
        GuestMemoryAccess::Rwx,
        false,
    )
    .expect("Failed to create guest memory");

    vm.create_vcpu(0).expect("Failed to create vcpu");

    let dtb = generate_fdt(&vm).expect("Failed to generate DT");
    vm.set_dtb_config(0x8000_0000, kib!(4), &dtb)
        .expect("Failed to set DTB configuration");

    assert_ok!(vm.start());
}
