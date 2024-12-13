// Copyright (c) 2024, Qualcomm Innovation Center, Inc. All rights reserved.
// SPDX-License-Identifier: BSD-3-Clause-Clear

use std::fs::File;
use std::num::NonZeroUsize;
use std::os::unix::io::{AsRawFd, FromRawFd, RawFd};

use gunyah_bindings::*;

use cfg_if::cfg_if;
#[cfg(feature = "ack-bindings")]
use memfd;
use nix::fcntl::{open, OFlag};
use nix::sys::stat::Mode;
use nix::unistd::dup;
use nix::NixPath;

use crate::guest_mem::GuestMem;
use crate::vm::Vm;
use crate::Result;

#[derive(Debug)]
pub struct Gunyah {
    gunyah: File,
}

impl Gunyah {
    /// Opens `/dev/gunyah` and returns a `Gunyah` object on success.
    ///
    /// # Example
    ///
    /// ```
    /// use gunyah::Gunyah;
    /// let gunyah = Gunyah::new().unwrap();
    /// ```
    #[allow(clippy::new_ret_no_self)]
    pub fn new() -> Result<Self> {
        // Open `/dev/gunyah` using `O_CLOEXEC` flag.
        Self::open_with_cloexec(true)
    }

    /// Opens the Gunyah device at `gunyah_path` and returns a `Gunyah` object on success.
    ///
    /// # Arguments
    ///
    /// * `gunyah_path`: path to the Gunyah device. Usually it is `/dev/gunyah`.
    ///
    /// # Example
    ///
    /// ```
    /// use gunyah::Gunyah;
    /// let gunyah = Gunyah::new_with_path("/dev/gunyah").unwrap();
    /// ```
    #[allow(clippy::new_ret_no_self)]
    pub fn new_with_path<P>(gunyah_path: &P) -> Result<Self>
    where
        P: ?Sized + NixPath,
    {
        // Open `gunyah_path` using `O_CLOEXEC` flag.
        Self::open_with_cloexec_at(gunyah_path, true)
    }

    /// Opens `/dev/gunyah` and returns the fd number on success.
    ///
    /// One usecase for this method is opening `/dev/gunyah` before exec-ing into a
    /// process with seccomp filters enabled that blacklist the `sys_open` syscall.
    /// For this usecase `open_with_cloexec` must be called with the `close_on_exec`
    /// parameter set to false.
    ///
    /// # Arguments
    ///
    /// * `close_on_exec`: If true opens `/dev/gunyah` using the `O_CLOEXEC` flag.
    ///
    /// # Example
    ///
    /// ```
    /// # use gunyah::Gunyah;
    /// let gunyah = Gunyah::open_with_cloexec(false);
    /// ```
    pub fn open_with_cloexec(close_on_exec: bool) -> Result<Self> {
        // SAFETY: Safe because we give a constant nul-terminated string.
        Self::open_with_cloexec_at("/dev/gunyah", close_on_exec)
    }

    /// Opens the Gunyah device at `gunyah_path` and returns the fd number on success.
    /// Same as [open_with_cloexec()](struct.Gunyah.html#method.open_with_cloexec)
    /// except this method opens `gunyah_path` instead of `/dev/gunyah`.
    ///
    /// # Arguments
    ///
    /// * `gunyah_path`: path to the Gunyah device. Usually it is `/dev/gunyah`.
    /// * `close_on_exec`: If true opens `gunyah_path` using the `O_CLOEXEC` flag.
    ///
    /// # Example
    ///
    /// ```
    /// # use gunyah::Gunyah;
    /// # use std::ffi::CString;
    /// # use std::os::unix::io::FromRawFd;
    /// let gunyah_path = CString::new("/dev/gunyah").unwrap();
    /// let gunyah_fd = Gunyah::open_with_cloexec_at(gunyah_path, false).unwrap();
    /// // The `gunyah_fd` can now be passed to another process where we can use
    /// // `from_raw_fd` for creating a `Gunyah` object:
    /// let gunyah = unsafe { Gunyah::from_raw_fd(gunyah_fd) };
    /// ```
    pub fn open_with_cloexec_at<P: ?Sized + NixPath>(
        path: &P,
        close_on_exec: bool,
    ) -> Result<Self> {
        let open_flags = OFlag::O_RDWR
            | if close_on_exec {
                OFlag::O_CLOEXEC
            } else {
                OFlag::empty()
            };
        let ret = open(path, open_flags, Mode::empty())?;
        // SAFETY: Safe because we know the path goes to a gunyah file
        Ok(unsafe { Self::from_raw_fd(ret) })
    }

    /// Creates a VM using the Gunyah fd of a specific type.
    ///
    /// See the documentation for `GUNYAH_CREATE_VM`.
    ///
    /// * `vm_type` - Platform and architecture specific platform VM type. A value of 0 is the equivalent
    ///               to using the default VM type.
    /// # Example
    ///
    /// ```
    /// # use gunyah::Gunyah;
    /// let gunyah = Gunyah::new().unwrap();
    /// let vm = gunyah.create_vm_with_type(0).unwrap();
    /// ```
    fn create_vm_with_type(&self, vm_type: i32) -> Result<Vm> {
        // SAFETY: Safe because we know `self.gunyah` is a real Gunyah fd as this module is the only one
        // that create Gunyah objects.
        let ret = unsafe { gunyah_create_vm(self.gunyah.as_raw_fd(), vm_type) }?;

        // SAFETY: Safe because we know gunyah_create_vm returns a file descriptor and we know it
        // returned successfully
        let file = unsafe { File::from_raw_fd(ret) };

        Ok(Vm::from(file))
    }

    /// Creates an unauthenticated VM using the Gunyah fd of a specific type.
    ///
    /// See the documentation for `GUNYAH_CREATE_VM`.
    ///
    /// * `vm_type` - Platform and architecture specific platform VM type. A value of 0 is the equivalent
    ///               to using the default VM type.
    /// # Example
    ///
    /// ```
    /// # use gunyah::Gunyah;
    /// let gunyah = Gunyah::new().unwrap();
    /// let vm = gunyah.create_vm().unwrap();
    /// ```
    pub fn create_vm(&self) -> Result<Vm> {
        self.create_vm_with_type(0)
    }

    cfg_if! {
        if #[cfg(not(feature = "ack-bindings"))] {
            /// Creates memory for Gunyah VMs.
            ///
            /// See the documentation for `GUNYAH_CREATE_GUEST_MEM`.
            ///
            /// * `flags` - Bitfield constructed from `gunyah_mem_flags`
            /// # Example
            ///
            /// ```
            /// # use gunyah::Gunyah;
            /// let gunyah = Gunyah::new().unwrap();
            /// let vm = gunyah.create_guest_memory(10485760, 0).unwrap();
            /// ```
            fn create_guest_memory_with_flags(&self, size: NonZeroUsize, flags: u64) -> Result<GuestMem> {
                let args = gunyah_create_mem_args {
                    size: u64::try_from(size.get()).map_err(|_| nix::Error::EINVAL)?,
                    flags,
                    ..Default::default()
                };

                // SAFETY: Safe because we know `self.gunyah` is a real Gunyah fd as this module is the only one
                // that create Gunyah objects.
                let ret = unsafe { gunyah_create_guest_mem(self.gunyah.as_raw_fd(), &args) }?;

                // SAFETY: Safe because we know gunyah_create_guest_mem returns a file descriptor and we
                // know the ioctl returned successfully
                let gmem_file = unsafe { File::from_raw_fd(ret) };
                Ok(GuestMem::from(gmem_file))
            }

            pub fn create_guest_memory(&self, size: NonZeroUsize, huge_pages: bool) -> Result<GuestMem> {
                self.create_guest_memory_with_flags(
                    size,
                    if huge_pages {
                        gunyah_mem_flags::GHMF_ALLOW_HUGEPAGE as u64
                    } else {
                        0u64
                    },
                )
            }

            pub fn create_guest_memory_with_cloexec(&self, size: NonZeroUsize) -> Result<GuestMem> {
                self.create_guest_memory_with_flags(size, gunyah_mem_flags::GHMF_CLOEXEC as u64)
            }
        } else {
            pub fn create_guest_memory(&self, size: NonZeroUsize, huge_pages: bool) -> Result<GuestMem> {
                let size = u64::try_from(size.get()).map_err(|_| nix::Error::EINVAL)?;
                let opts = memfd::MemfdOptions::default().allow_sealing(true);
                let mfd = opts.create("guest-mem").expect("Failed to create guest-mem");
                mfd.as_file().set_len(size).expect("Failed to set guest-mem length");
                Ok(GuestMem::from_file(mfd.into_file(), huge_pages))
            }

            pub fn create_guest_memory_with_cloexec(&self, size: NonZeroUsize) -> Result<GuestMem> {
                let size = u64::try_from(size.get()).map_err(|_| nix::Error::EINVAL)?;
                let opts = memfd::MemfdOptions::default().close_on_exec(true).allow_sealing(true);
                let mfd = opts.create("guest-mem").expect("Failed to create guest-mem");
                mfd.as_file().set_len(size).expect("Failed to set guest-mem length");

                Ok(GuestMem::from(mfd.into_file()))
            }
        }
    }

    pub fn dup(&self) -> nix::Result<Self> {
        // SAFETY: Safe because fd our fd is a Gunyah and the resulting dup'd
        // fd is also a Gunyah
        Ok(Self {
            gunyah: unsafe { File::from_raw_fd(dup(self.as_raw_fd())?) },
        })
    }
}

impl AsRawFd for Gunyah {
    fn as_raw_fd(&self) -> RawFd {
        self.gunyah.as_raw_fd()
    }
}

impl FromRawFd for Gunyah {
    /// Creates a new Gunyah object assuming `fd` represents an existing open file descriptor
    /// associated with `/dev/gunyah`.
    ///
    /// For usage examples check [open_with_cloexec()](struct.Gunyah.html#method.open_with_cloexec).
    ///
    /// # Arguments
    ///
    /// * `fd` - File descriptor for `/dev/gunyah`.
    ///
    /// # Safety
    ///
    /// This function is unsafe as the primitives currently returned have the contract that
    /// they are the sole owner of the file descriptor they are wrapping. Usage of this function
    /// could accidentally allow violating this contract which can cause memory unsafety in code
    /// that relies on it being true.
    ///
    /// The caller of this method must make sure the fd is valid and nothing else uses it.
    ///
    /// # Example
    ///
    /// ```
    /// # use gunyah::Gunyah;
    /// # use std::os::unix::io::FromRawFd;
    /// let gunyah_fd = Gunyah::open_with_cloexec(true).unwrap();
    /// // Safe because we verify that the fd is valid in `open_with_cloexec` and we own the fd.
    /// let gunyah = unsafe { Gunyah::from_raw_fd(gunyah_fd) };
    /// ```
    unsafe fn from_raw_fd(fd: RawFd) -> Self {
        Gunyah {
            gunyah: File::from_raw_fd(fd),
        }
    }
}

impl Clone for Gunyah {
    fn clone(&self) -> Self {
        self.dup()
            .unwrap_or_else(|_| panic!("Failed to dup {:?}", self.gunyah))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::undocumented_unsafe_blocks)]
    use super::*;
    use libc::{fcntl, FD_CLOEXEC, F_GETFD};

    macro_rules! mib {
        ($x:expr) => {
            $x * 1048576
        };
    }

    #[test]
    fn gunyah_new() {
        Gunyah::new().unwrap();
    }

    #[test]
    fn gunyah_new_with_path() {
        Gunyah::new_with_path("/dev/gunyah").unwrap();
    }

    #[test]
    fn open_with_cloexec() {
        let gunyah = Gunyah::open_with_cloexec(false).unwrap();
        let flags = unsafe { fcntl(gunyah.as_raw_fd(), F_GETFD, 0) };
        assert_eq!(flags & FD_CLOEXEC, 0);
        let gunyah = Gunyah::open_with_cloexec(true).unwrap();
        let flags = unsafe { fcntl(gunyah.as_raw_fd(), F_GETFD, 0) };
        assert_eq!(flags & FD_CLOEXEC, FD_CLOEXEC);
    }

    #[test]
    fn open_with_cloexec_at() {
        let gunyah = Gunyah::open_with_cloexec_at("/dev/gunyah", false).unwrap();
        let flags = unsafe { fcntl(gunyah.as_raw_fd(), F_GETFD, 0) };
        assert_eq!(flags & FD_CLOEXEC, 0);

        let gunyah = Gunyah::open_with_cloexec_at("/dev/gunyah", true).unwrap();
        let flags = unsafe { fcntl(gunyah.as_raw_fd(), F_GETFD, 0) };
        assert_eq!(flags & FD_CLOEXEC, FD_CLOEXEC);
    }

    #[test]
    fn create_vm() {
        let gunyah = Gunyah::new().unwrap();
        gunyah.create_vm().unwrap();
    }

    #[test]
    fn create_vm_with_type() {
        let gunyah = Gunyah::new().unwrap();
        gunyah.create_vm_with_type(0).unwrap();
    }

    #[test]
    fn create_mem() {
        let gunyah = Gunyah::new().unwrap();
        gunyah
            .create_guest_memory(NonZeroUsize::new(mib!(10)).unwrap(), false)
            .unwrap();
    }

    #[test]
    #[cfg(not(feature = "ack-bindings"))]
    fn create_mem_with_flags() {
        let gunyah = Gunyah::new().unwrap();
        gunyah
            .create_guest_memory_with_flags(NonZeroUsize::new(mib!(10)).unwrap(), 0)
            .unwrap();
    }

    #[test]
    #[cfg(not(feature = "ack-bindings"))]
    fn create_mem_with_cloexec() {
        let gunyah = Gunyah::new().unwrap();
        let mem = gunyah
            .create_guest_memory(NonZeroUsize::new(mib!(10)).unwrap(), false)
            .unwrap();
        let flags = unsafe { fcntl(mem.as_raw_fd(), F_GETFD, 0) };
        assert_eq!(flags & FD_CLOEXEC, 0);

        let gunyah = Gunyah::new().unwrap();
        let mem = gunyah
            .create_guest_memory_with_cloexec(NonZeroUsize::new(mib!(10)).unwrap())
            .unwrap();
        let flags = unsafe { fcntl(mem.as_raw_fd(), F_GETFD, 0) };
        assert_eq!(flags & FD_CLOEXEC, FD_CLOEXEC);
    }
}
