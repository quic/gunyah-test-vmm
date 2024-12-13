// Copyright (c) 2024, Qualcomm Innovation Center, Inc. All rights reserved.
// SPDX-License-Identifier: BSD-3-Clause-Clear

use std::{io::Read, os::fd::AsRawFd, time::Duration};

use claim::assert_none;
use gunyah_bindings::gunyah_vcpu_exit::GUNYAH_VCPU_EXIT_MMIO;
use mio::{unix::SourceFd, Events, Interest, Poll, Token};
use rstest::rstest;

use super::HoldingCell;

#[test]
fn basic_trigger() {
    let hc = HoldingCell::new();
    let address = 0x6_0000u64;
    let magic = 0xdeadu64;

    let mut ioevent = hc
        .vm
        .add_ioevent(address, 8, None)
        .expect("Failed to add ioeventfd");
    let fd = ioevent.as_file_mut();
    hc.write_io(0, address, magic)
        .expect("Failed to write to address");
    let mut data = [0u8; 8];
    fd.read_exact(&mut data).expect("Failed to read ioeventfd");
    assert_eq!(data[0], 1u8);
}

#[rstest]
fn datamatch(#[values(0, 0xf00d)] bad_magic: u64) {
    let hc = HoldingCell::new();
    let address = 0x6_0000u64;
    let magic = 0xdeadu64;
    let token = Token(1);

    let ioevent = hc
        .vm
        .add_ioevent(address, 8, Some(magic))
        .expect("Failed to add ioeventfd");

    let mut poll = Poll::new().expect("Failed to create poller");
    poll.registry()
        .register(
            &mut SourceFd(&ioevent.as_raw_fd()),
            token,
            Interest::WRITABLE,
        )
        .expect("Failed to register ioevent with poller");
    let mut events = Events::with_capacity(1);

    hc.write_io(0, address, magic)
        .expect("Failed to write to address");
    poll.poll(&mut events, Some(Duration::ZERO))
        .expect("Failed to poll");
    assert_eq!(
        events.iter().next().expect("No events received").token(),
        token
    );
    events.clear();

    // Ignore error, it's going to be confused we had MMIO exit
    let _ = hc.write_io(0, address, bad_magic);
    let state = hc.cell_state(0);
    assert_eq!(state.exit_reason, GUNYAH_VCPU_EXIT_MMIO);
    let mmio_exit = unsafe { state.__bindgen_anon_1.mmio };
    assert_eq!(mmio_exit.phys_addr, address);
    assert_eq!(mmio_exit.data, bad_magic.to_le_bytes());
    poll.poll(&mut events, Some(Duration::ZERO))
        .expect("Failed to poll");
    assert_none!(events.iter().next());
    events.clear();
}

#[test]
fn multiple_addresses() {
    let hc = HoldingCell::new();
    let addresses = [0x6_0000u64, 0x6_0008u64, 0x6_0010];
    let mut poll = Poll::new().expect("Failed to create poller");
    let mut events = Events::with_capacity(1);

    let ioevents = Vec::from_iter(addresses.iter().enumerate().map(|(index, address)| {
        let ioevent = hc
            .vm
            .add_ioevent(*address, 8, None)
            .expect("Failed to add ioeventfd");

        poll.registry()
            .register(
                &mut SourceFd(&ioevent.as_raw_fd()),
                Token(index),
                Interest::WRITABLE,
            )
            .expect("Failed to register ioevent with poller");

        ioevent
    }));

    for (index, address) in addresses.iter().enumerate() {
        hc.write_io(0, *address, 0)
            .expect("Failed to write to address");
        poll.poll(&mut events, Some(Duration::ZERO))
            .expect("Failed to poll");
        assert_eq!(
            events.iter().next().expect("No events received").token(),
            Token(index)
        );
    }

    drop(ioevents);
}

#[test]
fn multiple_datamatch() {
    let hc = HoldingCell::new();
    let address = 0x6_0000u64;
    let magic = [0x6_0000u64, 0x6_0008u64, 0x6_0010];
    let mut poll = Poll::new().expect("Failed to create poller");
    let mut events = Events::with_capacity(1);

    let ioevents = Vec::from_iter(magic.iter().enumerate().map(|(index, magic)| {
        let ioevent = hc
            .vm
            .add_ioevent(address, 8, Some(*magic))
            .expect("Failed to add ioeventfd");

        poll.registry()
            .register(
                &mut SourceFd(&ioevent.as_raw_fd()),
                Token(index),
                Interest::WRITABLE,
            )
            .expect("Failed to register ioevent with poller");

        ioevent
    }));

    for (index, magic) in magic.iter().enumerate() {
        hc.write_io(0, address, *magic)
            .expect("Failed to write to address");
        poll.poll(&mut events, Some(Duration::ZERO))
            .expect("Failed to poll");
        assert_eq!(
            events.iter().next().expect("No events received").token(),
            Token(index)
        );
    }

    drop(ioevents);
}
