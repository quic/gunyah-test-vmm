// Copyright (c) 2024, Qualcomm Innovation Center, Inc. All rights reserved.
// SPDX-License-Identifier: BSD-3-Clause-Clear

use std::{
    num::NonZeroUsize,
    thread,
    time::{Duration, Instant},
};

use claim::{assert_ok, assert_ok_eq};
use gunyah::GuestMemoryAccess;
use rstest::rstest;

use super::{HoldingCell, HoldingCellOptions};

#[derive(Debug, Clone, Copy)]
enum AffineStrategy {
    NoAffine,     // Don't affine
    AffineSame,   // Affine to same CPU
    AffineSpread, // Affine to different CPUs
}

#[derive(Debug, Clone, Copy)]
enum MemoryAmount {
    RegularPages(usize),
    HugePages(usize),
}

impl MemoryAmount {
    pub fn size(&self) -> usize {
        match self {
            MemoryAmount::RegularPages(size) => *size,
            MemoryAmount::HugePages(size) => *size,
        }
    }
}

/// Tests bring up of a second vCPU. The test is all single threaded,
/// so there are no possibility of race conditions and just verifies that
/// multicore is functional. We lend some memory to the cell and have
/// core 1 write to while core 0 reads it. Caching would be disabled by
/// default so no concerns about coherency.
#[rstest]
#[trace]
fn basic_multicore(#[values(2, 4, 7, 8, 16)] num_cells: u8) {
    let options = HoldingCellOptions {
        num_cells,
        ..Default::default()
    };
    let mut hc = HoldingCell::new_with_options(options);
    let address = 0xa000_0000u64;
    hc.vm
        .add_memory(
            address,
            NonZeroUsize::new(kib!(4)).unwrap(),
            gunyah::ShareType::Lend,
            GuestMemoryAccess::Rw,
            false,
        )
        .expect("Failed to add memory");

    // Explicitly start the VM in case it fails. Otherwise we get confused
    // that we couldn't do the PSCI call to turn on the other vCPU.
    if num_cells as usize <= core_affinity::get_core_ids().unwrap().len() {
        assert_ok!(hc.vm.start());
    } else {
        println!("{}", core_affinity::get_core_ids().unwrap().len());
        claim::assert_err!(hc.vm.start());
        return;
    }

    for cell in 1..num_cells {
        assert_ok!(hc.power_on_cell(cell), "Failed to power on vcpu {}", cell);
        let magic = cell as u64;
        assert_ok!(hc.write_addr(cell, address, magic));
        assert_ok_eq!(hc.read_addr(0, address), magic);
    }
}

#[test]
#[ignore = "share temporarily not working"]
fn share_reclaim_race_10sec() {
    let options = HoldingCellOptions {
        num_cells: 2,
        ..Default::default()
    };
    let mut hc = HoldingCell::new_with_options(options);
    let address = 0x0008_0000u64;
    let mem = hc
        .vm
        .add_memory(
            address,
            NonZeroUsize::new(kib!(4)).unwrap(),
            gunyah::ShareType::Share,
            GuestMemoryAccess::Rw,
            false,
        )
        .expect("Failed to add memory");
    let end = Instant::now() + Duration::from_secs(10);

    assert_ok!(hc.power_on_cell(1));

    let mut cell_0 = 0u64;
    let mut cell_1 = 0u64;
    let mut main = 0u64;
    thread::scope(|s| {
        s.spawn(|| {
            while Instant::now() < end {
                assert_ok!(hc.write_addr(0, address, 0xdeadf00d));
                cell_0 += 1;
            }
        });
        s.spawn(|| {
            while Instant::now() < end {
                assert_ok!(hc.write_addr(1, address, 0xdeadf00d));
                cell_1 += 1;
            }
        });

        while Instant::now() < end {
            assert_ok!(punch_hole!(mem, 0, kib!(4)));
            main += 1;
        }
    });

    println!(
        "punched hole {} times; cell 0 demanded {} times; cell 1 demanded {} times",
        main, cell_0, cell_1
    );
}

#[rstest]
#[trace]
fn large_footprint_race(
    #[values(MemoryAmount::RegularPages(mib!(10)), MemoryAmount::RegularPages(mib!(100))/*, MemoryAmount::RegularPages(mib!(1024)), MemoryAmount::HugePages(mib!(1024))*/)]
    amount: MemoryAmount,
    #[values(1, 2, 8)] num_cells: u8,
    #[values(
        AffineStrategy::NoAffine,
        AffineStrategy::AffineSame,
        AffineStrategy::AffineSpread
    )]
    affine: AffineStrategy,
) {
    let options = HoldingCellOptions {
        num_cells,
        ..Default::default()
    };
    let mut hc = HoldingCell::new_with_options(options);
    let address = 0xa000_0000u64;
    assert_ok!(hc.vm.add_memory(
        address,
        NonZeroUsize::new(amount.size()).unwrap(),
        gunyah::ShareType::Lend,
        GuestMemoryAccess::Rw,
        match amount {
            MemoryAmount::RegularPages(_) => false,
            MemoryAmount::HugePages(_) => true,
        },
    ));

    for index in 1..num_cells {
        assert_ok!(hc.power_on_cell(index), "Failed to start vcpu {}", index);
    }

    let hc = std::sync::Arc::new(hc);

    let start = Instant::now();
    thread::scope(|s| {
        let run = |index| {
            match affine {
                AffineStrategy::NoAffine => {}
                AffineStrategy::AffineSame => {
                    let core_ids = core_affinity::get_core_ids().unwrap();
                    core_affinity::set_for_current(*core_ids.first().unwrap());
                }
                AffineStrategy::AffineSpread => {
                    let core_ids = core_affinity::get_core_ids().unwrap();
                    core_affinity::set_for_current(
                        *core_ids.get(index as usize % core_ids.len()).unwrap(),
                    );
                }
            }
            hc.run_immediately(index, 7, &[address, amount.size() as u64])
        };
        for index in 0..num_cells {
            s.spawn(move || {
                assert_ok!(run(index));
            });
        }
    });

    println!("{:?}", Instant::now().duration_since(start));
}

#[test]
fn unclaimed_vcpu_exit() {
    let hc = HoldingCell::new();
    assert_ok!(hc.ack_ok(0));
    hc.vm
        .create_vcpu(1)
        .expect("Failed to create vcpu after vm was running");
    hc.power_off(0).unwrap();
}
