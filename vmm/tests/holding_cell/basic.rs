// Copyright (c) 2024, Qualcomm Innovation Center, Inc. All rights reserved.
// SPDX-License-Identifier: BSD-3-Clause-Clear

use claim::{assert_err, assert_lt, assert_ok, assert_ok_eq};
use gunyah_bindings::gunyah_vcpu_exit::GUNYAH_VCPU_EXIT_MMIO;
use rstest::rstest;

use crate::holding_cell::{HoldingCell, HOLDING_CELL_BIN};

use super::{generate_holding_cell_fdt, page_size, HoldingCellOptions};

/// Test that we can create a holding cell
#[test]
fn new_holding_cell() {
    HoldingCell::new();
}

#[test]
fn starts() {
    HoldingCell::new().vm.start().expect("Failed to start VM");
}

/// Test that we can start holding cell and vcpu faults at expected address
#[test]
fn holding_cell_reads() {
    let vm = HoldingCell::new();
    vm.vm.start().expect("Failed to start VM");
    let vcpu = &vm.vcpus[0];
    let result = vcpu.run_once().expect("vcpu run failed");
    assert_eq!(result.exit_reason, GUNYAH_VCPU_EXIT_MMIO);
    assert_eq!(unsafe { result.__bindgen_anon_1.mmio }.phys_addr, 0x6000);
    assert_eq!(unsafe { result.__bindgen_anon_1.mmio }.is_write, 0);
}

/// Test that we can run test_ok
#[test]
fn ok() {
    let hc = HoldingCell::new();
    assert_ok_eq!(hc.run_immediately(0, 0, &[]), 0);
    assert_ok!(hc.ack_ok(0));
}

/// Test that we can run test_ok
#[test]
fn nok() {
    let hc = HoldingCell::new();
    assert_ok_eq!(hc.run_immediately(0, 1, &[]), u64::MAX);
}

/// Test that we can run multiple basic tests on same VM
#[test]
fn multiple() {
    let hc = HoldingCell::new();
    for _ in 0..100 {
        assert_ok_eq!(hc.run_immediately(0, 0, &[]), 0);
        assert_ok_eq!(hc.run_immediately(0, 1, &[]), u64::MAX);
    }
}

// Verify read_addr works
#[test]
fn read_addr() {
    let hc = HoldingCell::new();
    let mut holding_cell_bytes = [0u8; 8];
    holding_cell_bytes.copy_from_slice(&HOLDING_CELL_BIN[0..8]);
    assert_ok_eq!(
        hc.read_addr(0, 0x8000_0000),
        u64::from_le_bytes(holding_cell_bytes)
    );
}

#[test]
fn loopback() {
    let hc = HoldingCell::new();
    let magic = 0x12345678u64;
    assert_ok_eq!(hc.run_immediately(0, 4, &[magic]), magic);
}

#[test]
fn magic() {
    let hc = HoldingCell::new();
    let magic = 0xdeadf00du64;
    assert_ok_eq!(hc.run_immediately(0, 5, &[magic]), 1);
}

#[test]
fn huge_pages_base() {
    let hc = HoldingCell::new_with_options(HoldingCellOptions {
        huge_pages: true,
        ..Default::default()
    });
    assert_ok_eq!(hc.run_immediately(0, 0, &[]), 0);
    assert_ok!(hc.ack_ok(0));
}

#[rstest]
fn big_dtb(#[values(true, false)] huge_pages: bool) {
    use nonzero_ext::NonZero;

    const ADDRESS: u64 = 0xa000_0000u64;

    let mut hc = HoldingCell::new();

    // generate the dtb
    let mut dtb = generate_holding_cell_fdt(&hc.vm, hc.vcpus.len() as u8).unwrap();
    let dtb_size = 4usize * page_size(huge_pages);
    assert_lt!(dtb.len(), dtb_size);
    dtb.resize(dtb_size, 0);

    // Add memory for the dtb
    assert_ok!(hc.vm.add_memory(
        ADDRESS,
        NonZero::new(dtb.len()).unwrap(),
        gunyah::ShareType::Lend,
        gunyah::GuestMemoryAccess::Rwx,
        huge_pages
    ));

    // install the dtb into guest memory
    assert_ok!(hc.vm.set_dtb_config(ADDRESS, dtb.len() as u64, &dtb));

    assert_ok!(hc.vm.start());
    assert_ok!(hc.ack_ok(0));
}

// Same as big_dtb, but guest memory acccess is Rw instead of Rwx
#[test]
fn bad_dtb_access() {
    use nonzero_ext::NonZero;

    const ADDRESS: u64 = 0xa000_0000u64;

    let mut hc = HoldingCell::new();

    // generate the dtb
    let mut dtb = generate_holding_cell_fdt(&hc.vm, hc.vcpus.len() as u8).unwrap();
    let dtb_size = 4usize * page_size(false);
    assert_lt!(dtb.len(), dtb_size);
    dtb.resize(dtb_size, 0);

    // Add memory for the dtb
    assert_ok!(hc.vm.add_memory(
        ADDRESS,
        NonZero::new(dtb.len()).unwrap(),
        gunyah::ShareType::Lend,
        gunyah::GuestMemoryAccess::Rw,
        false
    ));

    // install the dtb into guest memory
    assert_ok!(hc.vm.set_dtb_config(ADDRESS, dtb.len() as u64, &dtb));

    assert_err!(hc.vm.start());
}

// Same as big_dtb, but guest address is in MMIO range not in RAM (this test may pass one day)
#[test]
fn bad_dtb_addr() {
    use nonzero_ext::NonZero;

    const ADDRESS: u64 = 0x0008_0000u64;

    let mut hc = HoldingCell::new();

    // generate the dtb
    let mut dtb = generate_holding_cell_fdt(&hc.vm, hc.vcpus.len() as u8).unwrap();
    let dtb_size = 4usize * page_size(false);
    assert_lt!(dtb.len(), dtb_size);
    dtb.resize(dtb_size, 0);

    // Add memory for the dtb
    assert_ok!(hc.vm.add_memory(
        ADDRESS,
        NonZero::new(dtb.len()).unwrap(),
        gunyah::ShareType::Lend,
        gunyah::GuestMemoryAccess::Rw,
        false
    ));

    // install the dtb into guest memory
    assert_ok!(hc.vm.set_dtb_config(ADDRESS, dtb.len() as u64, &dtb));

    assert_err!(hc.vm.start());
}
