// Copyright (c) 2024, Qualcomm Innovation Center, Inc. All rights reserved.
// SPDX-License-Identifier: BSD-3-Clause-Clear

use std::{
    num::NonZeroUsize,
    os::fd::AsRawFd,
    thread,
    time::{Duration, Instant},
};

use claim::{assert_err, assert_ok, assert_ok_eq};
use gunyah::GuestMemoryAccess;
use rstest::rstest;

use crate::holding_cell::{FlushType, HoldingCell};

macro_rules! kib {
    ($x:expr) => {
        $x * 1024
    };
}

macro_rules! mib {
    ($x:expr) => {
        $x * 1048576
    };
}

/// Test that VM can access memory the host writes (LEND)
#[test]
// This test is only applicable with guest_memfd where it can enforce that
// userspace can't mmap/fault in the lent memory. In GUP case, we allow the
// fault but there would be S2 fault.
#[cfg(not(feature = "ack-bindings"))]
fn host_provided_lend() {
    const ADDRESS: u64 = 0xa000_0000u64;
    const MAGIC: &[u8; 8] = &0xf00du64.to_le_bytes();

    let mut hc = HoldingCell::new();
    hc.vm
        .add_memory(
            ADDRESS,
            NonZeroUsize::new(kib!(4)).unwrap(),
            gunyah::ShareType::Lend,
            GuestMemoryAccess::Rw,
            false,
        )
        .expect("Failed to add memory");

    // Write the magic from host
    hc.host_write_slice(ADDRESS, MAGIC)
        .expect("failed to write magic to be read");

    let mut data = [0u8; 8];
    // Make sure host can read same thing back.
    assert_ok!(hc.host_read_slice(ADDRESS, &mut data));
    assert_eq!(data, *MAGIC);

    // Make sure guest can read it back
    assert_ok_eq!(hc.read_addr(0, ADDRESS), u64::from_le_bytes(*MAGIC));

    // Host can't read anymore
    assert_err!(hc.host_read_slice(ADDRESS, &mut data));
}

/// Test that VM can access memory the host writes (SHARE)
#[test]
fn host_provided_share() {
    const ADDRESS: u64 = 0x0008_0000u64;
    const MAGIC: &[u8; 8] = &0xf00du64.to_le_bytes();

    let mut hc = HoldingCell::new();
    hc.vm
        .add_memory(
            ADDRESS,
            NonZeroUsize::new(kib!(4)).unwrap(),
            gunyah::ShareType::Share,
            GuestMemoryAccess::Rw,
            false,
        )
        .expect("Failed to add memory");

    hc.host_write_slice(ADDRESS, MAGIC)
        .expect("failed to write magic to be read");

    let mut data = [0u8; 8];
    // Make sure host can read same thing back.
    assert_ok!(hc.host_read_slice(ADDRESS, &mut data));
    assert_eq!(data, *MAGIC);

    // Make sure guest can read it back
    assert_ok_eq!(hc.read_addr(0, ADDRESS), u64::from_le_bytes(*MAGIC));

    let mut data = [0u8; 8];
    // Make sure host can read same thing back.
    assert_ok!(hc.host_read_slice(ADDRESS, &mut data));
    assert_eq!(data, *MAGIC);
}

/// Test that VM can access memory via a write (LEND)
#[test]
// This test is only applicable with guest_memfd where it can enforce that
// userspace can't mmap/fault in the lent memory. In GUP case, we allow the
// fault but there would be S2 fault.
#[cfg(not(feature = "ack-bindings"))]
fn guest_writes_lend() {
    const ADDRESS: u64 = 0xa000_0000u64;
    const MAGIC: u64 = 0xdeadf00d;

    let mut hc = HoldingCell::new();
    hc.vm
        .add_memory(
            ADDRESS,
            NonZeroUsize::new(kib!(4)).unwrap(),
            gunyah::ShareType::Lend,
            GuestMemoryAccess::Rw,
            false,
        )
        .expect("Failed to add memory");

    // Guest writes
    assert_ok!(hc.write_addr(0, ADDRESS, MAGIC));

    // Make sure host cannot read it.
    let mut data = [0u8; 8];
    assert_err!(hc.host_read_slice(ADDRESS, &mut data));

    // Make sure the guest can still read.
    assert_ok_eq!(hc.read_addr(0, ADDRESS), MAGIC);
}

/// Test that VM can access memory via a write (SHARE)
#[test]
fn guest_writes_share() {
    const ADDRESS: u64 = 0x0008_0000u64;
    const MAGIC: u64 = 0xdeadf00d;

    let mut hc = HoldingCell::new();
    hc.vm
        .add_memory(
            ADDRESS,
            NonZeroUsize::new(kib!(4)).unwrap(),
            gunyah::ShareType::Share,
            GuestMemoryAccess::Rw,
            false,
        )
        .expect("Failed to add memory");

    assert_ok!(hc.write_addr(0, ADDRESS, MAGIC));

    // Make sure host can read it, too.
    let mut data = [0u8; 8];
    assert_ok!(hc.host_read_slice(ADDRESS, &mut data));
    assert_eq!(u64::from_le_bytes(data), MAGIC);

    // And that guest can still read.
    assert_ok_eq!(hc.read_addr(0, ADDRESS), MAGIC);
}

#[test]
fn guest_share_coherency() {
    const ADDRESS: u64 = 0x0008_0000u64;
    const MAGIC: [u64; 2] = [0xf00ddead, 0xa5a5a5a5];

    let mut hc = HoldingCell::new();
    hc.vm
        .add_memory(
            ADDRESS,
            NonZeroUsize::new(kib!(4)).unwrap(),
            gunyah::ShareType::Share,
            GuestMemoryAccess::Rw,
            false,
        )
        .expect("Failed to add memory");

    // guest does the write, host should be able to see it
    for magic in MAGIC {
        println!("Guest writes {:x}", magic);
        assert_ok!(hc.write_addr(0, ADDRESS, magic));
        // Make sure host can read it, too.
        let mut data = [0u8; 8];
        assert_ok!(hc.host_read_slice(ADDRESS, &mut data));
        assert_eq!(u64::from_le_bytes(data), magic);
        // And that guest can still read.
        assert_ok_eq!(hc.read_addr(0, ADDRESS), magic);
    }

    // host does the write, guest should be able to see it
    for magic in MAGIC {
        println!("Host writes {:x}", magic);
        assert_ok!(hc.host_write_slice(ADDRESS, &magic.to_le_bytes()));
        // Make sure host can read it, too.
        let mut data = [0u8; 8];
        assert_ok!(hc.host_read_slice(ADDRESS, &mut data));
        assert_eq!(u64::from_le_bytes(data), magic);
        // And that guest can still read.
        assert_ok_eq!(hc.read_addr(0, ADDRESS), magic);
    }
}

#[test]
fn offset_paging() {
    const ADDRESS: u64 = 0x0008_0000u64;
    const NUM_PAGES: u64 = 10;
    const STRIDE: u64 = 0x600;
    const ITERS: u64 = (NUM_PAGES * kib!(4)) / STRIDE;

    let mut hc = HoldingCell::new();
    let mem = hc
        .vm
        .add_memory(
            ADDRESS,
            NonZeroUsize::new((NUM_PAGES * kib!(4)) as usize).unwrap(),
            gunyah::ShareType::Share,
            GuestMemoryAccess::Rw,
            false,
        )
        .expect("Failed to add memory");

    hc.vm.start().expect("Failed to start vm");

    let mem = mem
        .lock()
        .expect("Failed to lock mem")
        .as_region()
        .map()
        .expect("Failed to map region");

    for i in 0..ITERS {
        println!("Guest writes {:x}", i);
        assert_ok!(hc.write_addr(0, ADDRESS + (i * STRIDE), i));
    }

    for i in 0..ITERS {
        assert_eq!(mem[(i * STRIDE) as usize], i as u8);
    }
}

#[test]
#[cfg(not(feature = "ack-bindings"))]
fn share_punch_hole_10k_iters() {
    const ADDRESS: u64 = 0x0008_0000u64;
    const MAGIC: u64 = 0xdeadf00d;

    let mut hc = HoldingCell::new();
    let mem = hc
        .vm
        .add_memory(
            ADDRESS,
            NonZeroUsize::new(kib!(4)).unwrap(),
            gunyah::ShareType::Share,
            GuestMemoryAccess::Rw,
            false,
        )
        .expect("Failed to add memory");
    for i in 0..10000 {
        // Write, make sure it wrote, punch hole, read 0. And repeat.
        assert_ok!(hc.write_addr(0, ADDRESS, MAGIC + i));
        let mut data = [0u8; 8];
        assert_ok!(hc.host_read_slice(ADDRESS, &mut data));
        assert_eq!(data, (MAGIC + i).to_le_bytes());
        assert_ok!(punch_hole!(mem, 0, kib!(4)));
        assert_ok_eq!(hc.read_addr(0, ADDRESS), 0);
    }
}

#[test]
// This test is only applicable with guest_memfd where it can enforce that
// userspace can't mmap/fault in the lent memory. In GUP case, we allow the
// fault but there would be S2 fault.
#[cfg(not(feature = "ack-bindings"))]
fn lend_punch_hole_fails() {
    const ADDRESS: u64 = 0xa000_0000u64;
    const MAGIC: u64 = 0xdeadf00d;

    let mut hc = HoldingCell::new();
    let mem = hc
        .vm
        .add_memory(
            ADDRESS,
            NonZeroUsize::new(kib!(4)).unwrap(),
            gunyah::ShareType::Lend,
            GuestMemoryAccess::Rw,
            false,
        )
        .expect("Failed to add memory");

    // Write magic to lock the page
    assert_ok!(hc.write_addr(0, ADDRESS, MAGIC));

    // make sure host can't free the page
    assert_err!(punch_hole!(mem, 0, kib!(4)));

    // Make sure the host really didn't free the page: guest can still access
    assert_ok_eq!(hc.read_addr(0, ADDRESS), MAGIC);
}

#[test]
// This test is only applicable with guest_memfd where it can enforce that
// userspace can't mmap/fault in the lent memory. In GUP case, we allow the
// fault but there would be S2 fault.
#[cfg(not(feature = "ack-bindings"))]
fn lend_unlock_no_sanitize() {
    const ADDRESS: u64 = 0xa000_0000u64;
    const MAGIC: u64 = 0xf00ddeadu64;

    let mut hc = HoldingCell::new();
    let mut data = [0u8; 8];
    let mem = hc
        .vm
        .add_memory(
            ADDRESS,
            NonZeroUsize::new(kib!(4)).unwrap(),
            gunyah::ShareType::Lend,
            GuestMemoryAccess::Rw,
            false,
        )
        .expect("Failed to add memory");

    // Demand page the address
    assert_ok!(hc.write_addr(0, ADDRESS, MAGIC));
    // Relinquish the page. We don't need to flush because no S1 MMU
    assert_ok!(hc.page_relinquish(0, ADDRESS, 1, false, FlushType::NoFlush));

    // Make sure host can read the value (note: we didn't sanitize)
    assert_ok!(hc.host_read_slice(ADDRESS, &mut data));
    assert_eq!(data, MAGIC.to_le_bytes());

    // Free the page
    assert_ok!(punch_hole!(mem, 0, kib!(4)));

    // Make sure guest reads a new, free page
    assert_ok_eq!(hc.read_addr(0, ADDRESS), 0);
}

#[test]
#[ignore = "No support yet for guest immediately reclaiming page. HA was unset"]
fn unlocked_page_access() {
    let mut hc = HoldingCell::new();
    let address = 0xa000_0000u64;
    let mem = hc
        .vm
        .add_memory(
            address,
            NonZeroUsize::new(kib!(4)).unwrap(),
            gunyah::ShareType::Lend,
            GuestMemoryAccess::Rw,
            false,
        )
        .expect("Failed to add memory");
    assert_ok!(hc.write_addr(0, address, 0xdeadf00d));
    assert_ok!(hc.page_relinquish(0, address, 1, false, FlushType::FlushOnLast));
    assert_ok_eq!(hc.read_addr(0, address), 0xdeadf00d);
    assert_err!(punch_hole!(mem, 0, kib!(4))); // Supposed to fail as VM accessed the page before host reclaimed the page
}

#[test]
// todo
#[cfg(not(feature = "ack-bindings"))]
fn partial_page_reclaim() {
    let mut hc = HoldingCell::new();
    let address = 0xa000_0000u64;
    let mem = hc
        .vm
        .add_memory(
            address,
            NonZeroUsize::new(mib!(4)).unwrap(),
            gunyah::ShareType::Lend,
            GuestMemoryAccess::Rw,
            true,
        )
        .expect("Failed to add memory");
    assert_ok!(hc.write_addr(0, address, 0xdeadf00d));
    assert_ok_eq!(hc.read_addr(0, address), 0xdeadf00d);
    assert_ok!(hc.page_relinquish(
        0,
        address,
        256,
        false,
        crate::holding_cell::FlushType::NoFlush
    ));
    assert_err!(punch_hole!(mem, 0, kib!(4))); // Supposed to fail as we partially unlocked the huge page
    assert_ok!(hc.page_relinquish(0, address, 512, false, FlushType::NoFlush));
    assert_ok!(punch_hole!(mem, 0, kib!(4)));
    assert_ok!(punch_hole!(mem, kib!(4), mib!(2)));
    assert_ok!(punch_hole!(mem, 0, mib!(2)));
    assert_ok!(hc.read_addr(0, address));
}

#[test]
// This test is only applicable with guest_memfd where it can enforce that
// userspace can't mmap/fault in the lent memory. In GUP case, we allow the
// fault but there would be S2 fault.
#[cfg(not(feature = "ack-bindings"))]
fn sanitize_page() {
    let mut hc = HoldingCell::new();
    let address = 0xa000_0000u64;
    let mem = hc
        .vm
        .add_memory(
            address,
            NonZeroUsize::new(kib!(4)).unwrap(),
            gunyah::ShareType::Lend,
            GuestMemoryAccess::Rw,
            false,
        )
        .expect("Failed to add memory");
    assert_ok!(hc.write_addr(0, address, 0xdeadf00d));
    assert_ok_eq!(hc.read_addr(0, address), 0xdeadf00d);
    let mut data = [0u8; 8];
    assert_err!(hc.vm.read_slice(address, &mut data));
    assert_ok!(hc.page_relinquish(0, address, 1, true, FlushType::FlushOnLast));

    let mut data = [0u8; 8];
    assert_ok!(hc.vm.read_slice(address, &mut data));
    let value = u64::from_le_bytes(data[..std::mem::size_of::<u64>()].try_into().unwrap());
    assert_eq!(value, 0);
    assert_ok!(punch_hole!(mem, 0, kib!(4)));
    assert_ok!(hc.read_addr(0, address));
}

#[test]
// This test is only applicable with guest_memfd where it can enforce that
// userspace can't mmap/fault in the lent memory. In GUP case, we allow the
// fault but there would be S2 fault.
#[cfg(not(feature = "ack-bindings"))]
fn lend_no_access_before() {
    let mut hc = HoldingCell::new();
    let address = 0xa000_0000u64;

    let mem = hc
        .vm
        .add_memory(
            address,
            NonZeroUsize::new(kib!(4)).unwrap(),
            gunyah::ShareType::Lend,
            GuestMemoryAccess::Rw,
            false,
        )
        .expect("Failed to add memory");

    let mut data = [0u8; 8];
    let mapping = mem
        .lock()
        .unwrap()
        .as_region()
        .map_region(0, NonZeroUsize::new(data.len()).unwrap())
        .expect("Failed to mmap before VM starts");
    data.copy_from_slice(&mapping);
    assert_err!(hc.write_addr(0, address, 0xf00ddead));
    drop(mapping);
}

#[test]
// This test is only applicable with guest_memfd where it can enforce that
// userspace can't mmap/fault in the lent memory. In GUP case, we allow the
// fault but there would be S2 fault.
#[cfg(not(feature = "ack-bindings"))]
fn lend_no_access_after() {
    let mut hc = HoldingCell::new();
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

    assert_ok!(hc.write_addr(0, address, 0xf00ddead));

    let mut data = [0u8; 8];
    assert_err!(hc.vm.read_slice(address, &mut data));
    assert_ne!(data, 0xf00ddeadu64.to_le_bytes());
}

#[test]
#[ignore = "share temporarily not working"]
fn share_reclaim_race_10sec() {
    let mut hc = HoldingCell::new();
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
    thread::scope(|s| {
        s.spawn(|| {
            while Instant::now() < end {
                assert_ok!(hc.write_addr(0, address, 0xdeadf00d));
            }
        });

        while Instant::now() < end {
            assert_ok!(punch_hole!(mem, 0, kib!(4)));
        }
    });
}

/// Test fast access to various sizes and types of guest memfd
/// With regular page size, test 1 MB and 10 MB sizes
/// With huges pages, test 4 MB, 100 MB, and 1GB sizes
/// Holding cell accesses each page very quickly
/// Overall perf will be doubled since test case time would include tear-down
/// time. Test stdout will print the wall time to lend memory
#[rstest]
#[case(mib!(1), false)] // case 1
#[case(mib!(10), false)] // case 2
#[case(mib!(4), true)] // case 3
// #[case(mib!(100), true)] // case 4
// #[case(mib!(1024), true)] // case 5
#[trace]
fn large_footprint(#[case] size: usize, #[case] huge_pages: bool) {
    let mut hc = HoldingCell::new();
    let address = 0xa000_0000u64;
    assert_ok!(hc.vm.add_memory(
        address,
        NonZeroUsize::new(size).unwrap(),
        gunyah::ShareType::Lend,
        GuestMemoryAccess::Rw,
        huge_pages,
    ));
    let start = Instant::now();
    assert_ok!(hc.run_immediately(0, 7, &[address, size as u64]));
    println!("{:?}", Instant::now().duration_since(start));
}

// To test that unaligned access is ok, apply the patch below. This ioctl
// doesn't make sense in production, so it won't be merged anywhere. The
// pr_err will print some mostly garbage value. We don't care what it prints:
// kernel should not crash.
// We don't own the other page either and we assume it's accessible, so
// there's a small chance that the adjacent page is unmapped at S2 but not in S1.
// that shouldn't be possible as I write this, but if you're here in the future
// -- something to check. If our driver is properly written and someone else
// buggy,then this test would occasionally fail whenver the adjacent page isn't
// properly unmapped by the other buggy entity.
// ---
// diff --git a/drivers/virt/gunyah/guest_memfd.c b/drivers/virt/gunyah/guest_memfd.c
// index 88ef2c45a97c..bd1c7f7dd634 100644
// --- a/drivers/virt/gunyah/guest_memfd.c
// +++ b/drivers/virt/gunyah/guest_memfd.c
// @@ -5,6 +5,8 @@
//
//  #define pr_fmt(fmt) "gunyah_guest_mem: " fmt
//
// +#include <asm/word-at-a-time.h>
// +
//  #include <linux/anon_inodes.h>
//  #include <linux/types.h>
//  #include <linux/falloc.h>
// @@ -333,6 +335,23 @@ static int gunyah_gmem_release(struct inode *inode, struct file *file)
//         return 0;
//  }
//
// +static long gunyah_gmem_ioctl(struct file *filp, unsigned int cmd, unsigned long arg)
// +{
// +       switch (cmd) {
// +       case 0x200: {
// +               struct folio *folio = gunyah_gmem_get_folio(file_inode(filp), 0);
// +               void *ptr = folio_address(folio);
// +
// +               pr_err("%s:%d %x\n", __func__, __LINE__, load_unaligned_zeropad(ptr - 4));
// +               folio_unlock(folio);
// +               folio_put(folio);
// +               return 0;
// +       }
// +       default:
// +               return -ENOTTY;
// +       }
// +}
// +
//  static const struct file_operations gunyah_gmem_fops = {
//         .owner = THIS_MODULE,
//         .llseek = generic_file_llseek,
// @@ -340,6 +359,7 @@ static const struct file_operations gunyah_gmem_fops = {
//         .open = generic_file_open,
//         .fallocate = gunyah_gmem_fallocate,
//         .release = gunyah_gmem_release,
// +       .unlocked_ioctl = gunyah_gmem_ioctl,
//  };
//
//  static bool gunyah_gmem_release_folio(struct folio *folio, gfp_t gfp_flags)
#[test]
#[ignore = "The ioctl to test won't be merged"]
fn adjacent_unaligned_access_ok() {
    use libc::ioctl;

    const ADDRESS: u64 = 0xa000_0000u64;
    const MAGIC: &[u8; 8] = &0xf00du64.to_le_bytes();

    let mut hc = HoldingCell::new();
    let mem = hc
        .vm
        .add_memory(
            ADDRESS,
            NonZeroUsize::new(kib!(4)).unwrap(),
            gunyah::ShareType::Lend,
            GuestMemoryAccess::Rw,
            false,
        )
        .expect("Failed to add memory");

    // Write the magic from host
    hc.host_write_slice(ADDRESS, MAGIC)
        .expect("failed to write magic to be read");

    let mut data = [0u8; 8];
    // Make sure host can read same thing back.
    assert_ok!(hc.host_read_slice(ADDRESS, &mut data));
    assert_eq!(data, *MAGIC);

    // Make sure guest can read it back
    assert_ok_eq!(hc.read_addr(0, ADDRESS), u64::from_le_bytes(*MAGIC));

    // Host can't read anymore
    assert_err!(hc.host_read_slice(ADDRESS, &mut data));

    assert_eq!(0, unsafe {
        ioctl(
            mem.lock().unwrap().as_region().as_guest_mem().as_raw_fd(),
            0x200,
        )
    });
}

#[test]
// todo
#[cfg(not(feature = "ack-bindings"))]
fn partial_unmap_memory1() {
    const ADDRESS: u64 = 0x0008_0000u64;

    let mut hc = HoldingCell::new();
    let mem = hc
        .vm
        .add_memory(
            ADDRESS,
            NonZeroUsize::new(5 * kib!(4)).unwrap(),
            gunyah::ShareType::Share,
            GuestMemoryAccess::Rw,
            false,
        )
        .expect("Failed to add memory");

    assert_ok!(hc.host_write_slice(ADDRESS, &1u64.to_le_bytes()));
    assert_ok_eq!(hc.read_addr(0, ADDRESS), 1u64);

    assert_ok!(hc.host_write_slice(ADDRESS + kib!(4), &2u64.to_le_bytes()));
    assert_ok_eq!(hc.read_addr(0, ADDRESS + kib!(4)), 2u64);

    hc.vm
        .punch_hole(mem, 0, kib!(4))
        .expect("Failed to punch hole");

    assert_ok_eq!(hc.read_io(0, ADDRESS, 0xf00d), 0xf00du64);
    assert_ok_eq!(hc.read_addr(0, ADDRESS + kib!(4)), 2u64);

    let mut data = [0u8; 8];
    assert_err!(hc.host_read_slice(ADDRESS, &mut data));

    let mut data = [0u8; 8];
    assert_ok!(hc.host_read_slice(ADDRESS + kib!(4), &mut data));
    assert_eq!(data, 2u64.to_le_bytes());
}

#[test]
// todo
#[cfg(not(feature = "ack-bindings"))]
fn partial_unmap_memory2() {
    const ADDRESS: u64 = 0x0008_0000u64;

    let mut hc = HoldingCell::new();
    let mem = hc
        .vm
        .add_memory(
            ADDRESS,
            NonZeroUsize::new(5 * kib!(4)).unwrap(),
            gunyah::ShareType::Share,
            GuestMemoryAccess::Rw,
            false,
        )
        .expect("Failed to add memory");

    assert_ok!(hc.host_write_slice(ADDRESS, &1u64.to_le_bytes()));
    assert_ok_eq!(hc.read_addr(0, ADDRESS), 1u64);

    assert_ok!(hc.host_write_slice(ADDRESS + kib!(4), &2u64.to_le_bytes()));
    assert_ok_eq!(hc.read_addr(0, ADDRESS + kib!(4)), 2u64);

    hc.vm
        .punch_hole(mem, kib!(4), 4 * kib!(4))
        .expect("Failed to punch hole");

    assert_ok_eq!(hc.read_addr(0, ADDRESS), 1u64);
    assert_ok_eq!(hc.read_io(0, ADDRESS + kib!(4), 0xf00d), 0xf00du64);

    let mut data = [0u8; 8];
    assert_ok!(hc.host_read_slice(ADDRESS, &mut data));
    assert_eq!(data, 1u64.to_le_bytes());

    let mut data = [0u8; 8];
    assert_err!(hc.host_read_slice(ADDRESS + kib!(4), &mut data));
}
