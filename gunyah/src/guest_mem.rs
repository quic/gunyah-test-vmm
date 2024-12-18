// Copyright (c) 2024, Qualcomm Innovation Center, Inc. All rights reserved.
// SPDX-License-Identifier: BSD-3-Clause-Clear

use std::{
    fs::File,
    io,
    num::NonZeroUsize,
    os::fd::{AsRawFd, FromRawFd, RawFd},
};

use anyhow::anyhow;
use libc::{c_int, off_t};
pub use memmap::Mmap;
use memmap::{MmapMut, MmapOptions};
use nix::unistd::dup;
use same_file::Handle;

#[derive(Debug, PartialEq, Eq, Hash)]
pub struct GuestMem(Handle, #[cfg(feature = "ack-bindings")] bool);

impl GuestMem {
    pub fn allocate(&self, offset: off_t, len: off_t) -> nix::Result<()> {
        const FLAGS: c_int = libc::FALLOC_FL_KEEP_SIZE;
        let res = unsafe { libc::fallocate(self.as_raw_fd(), FLAGS, offset, len) };
        nix::errno::Errno::result(res).map(drop)
    }

    pub fn punch_hole(&self, offset: off_t, len: off_t) -> nix::Result<()> {
        const FLAGS: c_int = libc::FALLOC_FL_KEEP_SIZE | libc::FALLOC_FL_PUNCH_HOLE;
        let res = unsafe { libc::fallocate(self.as_raw_fd(), FLAGS, offset, len) };
        nix::errno::Errno::result(res).map(drop)
    }

    pub fn dup(&self) -> nix::Result<Self> {
        // SAFETY: Safe because fd our fd is a GuestMem and the resulting dup'd
        // fd is also a GuestMem
        let file = unsafe { File::from_raw_fd(dup(self.as_raw_fd())?) };
        Ok(Self(
            Handle::from_file(file).map_err(|e| {
                e.raw_os_error()
                    .map_or(nix::Error::UnknownErrno, nix::Error::from_raw)
            })?,
            #[cfg(feature = "ack-bindings")]
            self.1,
        ))
    }

    pub fn as_file(&self) -> &File {
        self.0.as_file()
    }

    pub fn as_file_mut(&mut self) -> &mut File {
        self.0.as_file_mut()
    }

    pub fn into_file(self) -> File {
        // SAFETY: Safe because fd our fd is a GuestMem and the resulting dup'd
        // fd is also a GuestMem
        unsafe { File::from_raw_fd(dup(self.as_raw_fd()).expect("Unable to dup guest mem")) }
    }

    #[cfg(feature = "ack-bindings")]
    pub fn from_file(file: File, huge_pages: bool) -> Self {
        Self(
            Handle::from_file(file).expect("Unable to get info about file"),
            huge_pages,
        )
    }

    #[cfg(feature = "ack-bindings")]
    pub fn use_huge_pages(&self) -> bool {
        self.1
    }
}

impl AsRawFd for GuestMem {
    fn as_raw_fd(&self) -> RawFd {
        self.0.as_raw_fd()
    }
}

impl From<File> for GuestMem {
    fn from(file: File) -> Self {
        Self(
            Handle::from_file(file).expect("Unable to get info about file"),
            #[cfg(feature = "ack-bindings")]
            false,
        )
    }
}

impl Clone for GuestMem {
    fn clone(&self) -> Self {
        self.dup()
            .unwrap_or_else(|_| panic!("Failed to dup {:?}", self.0))
    }
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct GuestMemRegion {
    mem: GuestMem,
    off: u64,
    size: NonZeroUsize,
}

impl GuestMemRegion {
    pub fn new(mem: GuestMem, off: u64, size: NonZeroUsize) -> anyhow::Result<Self> {
        if off as usize + size.get() > mem.as_file().metadata()?.len().try_into()? {
            return Err(anyhow!("GuestMemRegion extents past end of GuestMem"));
        }
        Ok(Self { mem, off, size })
    }

    fn map_options(&self, off: u64, size: NonZeroUsize) -> io::Result<MmapOptions> {
        if off as usize + size.get() > self.size.get() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Offset and size incorrect",
            ));
        }
        Ok(MmapOptions::new()
            .offset(self.off + off)
            .len(size.get())
            .to_owned())
    }

    pub fn map_region(&self, off: u64, size: NonZeroUsize) -> io::Result<Mmap> {
        // SAFETY: Safe because we know we have a Gunyah guestmemfd
        unsafe { self.map_options(off, size)?.map(self.mem.as_file()) }
    }

    pub fn map_region_mut(&self, off: u64, size: NonZeroUsize) -> io::Result<MmapMut> {
        // SAFETY: Safe because we know we have a Gunyah guestmemfd
        unsafe { self.map_options(off, size)?.map_mut(self.mem.as_file()) }
    }

    pub fn map(&self) -> io::Result<Mmap> {
        self.map_region(0, self.size)
    }

    pub fn map_mut(&self) -> io::Result<MmapMut> {
        self.map_region_mut(0, self.size)
    }

    pub fn as_guest_mem(&self) -> &GuestMem {
        &self.mem
    }

    pub fn offset(&self) -> u64 {
        self.off
    }

    pub fn size(&self) -> usize {
        self.size.get()
    }
}

#[cfg(all(test, not(feature = "ack-bindings")))]
mod tests {
    #![allow(clippy::undocumented_unsafe_blocks)]

    use std::num::NonZeroUsize;

    use claim::*;

    use crate::gunyah::Gunyah;

    use super::GuestMemRegion;

    macro_rules! mib {
        ($x:expr) => {
            $x * 1048576
        };
    }

    #[test]
    fn fallocate() {
        let gunyah = Gunyah::new().unwrap();
        let gmem = gunyah
            .create_guest_memory(NonZeroUsize::new(mib!(4)).unwrap(), false)
            .unwrap();
        assert_ok!(gmem.allocate(0, mib!(4)));
        assert_err!(gmem.allocate(0, 1));
        assert_err!(gmem.allocate(1, mib!(4)));
        assert_err!(gmem.allocate(1, mib!(10)));
        assert_ok!(gmem.allocate(0, mib!(1)));
        assert_ok!(gmem.allocate(mib!(2), mib!(1)));
    }

    #[test]
    fn setlen() {
        let gunyah = Gunyah::new().unwrap();
        let gmem = gunyah
            .create_guest_memory(NonZeroUsize::new(mib!(4)).unwrap(), false)
            .unwrap();
        assert_ok!(gmem.as_file().set_len(mib!(100)));
        assert_ok!(gmem.as_file().set_len(mib!(50)));
        assert_ok!(gmem.allocate(mib!(49), mib!(1)));
        assert_err!(gmem.allocate(mib!(50), mib!(1)));
        assert_ok!(gmem.as_file().set_len(mib!(10)));
    }

    #[test]
    fn punch_hole() {
        let gunyah = Gunyah::new().unwrap();
        let gmem = gunyah
            .create_guest_memory(NonZeroUsize::new(mib!(4)).unwrap(), false)
            .unwrap();
        // These should do nothing; test that kernel is happy
        assert_ok!(gmem.punch_hole(mib!(1), mib!(1)));
        assert_ok!(gmem.punch_hole(0, mib!(1)));

        assert_ok!(gmem.as_file().set_len(mib!(4)));

        // This should do nothing; test that kernel is happy
        assert_ok!(gmem.punch_hole(mib!(5), mib!(1)));

        // Allocate all the memory for the guest mem
        assert_ok!(gmem.allocate(0, mib!(4)));

        // Punch it
        assert_ok!(gmem.punch_hole(mib!(1), mib!(1)));
    }

    #[test]
    fn dup() {
        let gunyah = Gunyah::new().unwrap();
        let gmem = gunyah
            .create_guest_memory(NonZeroUsize::new(4096).unwrap(), false)
            .unwrap();
        let dupd = gmem.dup().unwrap();

        assert_err!(gmem.allocate(0, mib!(1)));
        assert_err!(dupd.allocate(0, mib!(1)));

        assert_ok!(gmem.as_file().set_len(mib!(1)));
        assert_ok!(dupd.allocate(0, mib!(1)));
    }

    #[test]
    fn mmap() {
        let gunyah = Gunyah::new().unwrap();
        let gmem = gunyah
            .create_guest_memory(NonZeroUsize::new(4096).unwrap(), false)
            .unwrap();
        let region = GuestMemRegion::new(gmem, 0, NonZeroUsize::new(4096).unwrap()).unwrap();
        let _mmap = region.map().unwrap();
        assert_eq!(_mmap[..], [0u8; 4096]);
    }
}
