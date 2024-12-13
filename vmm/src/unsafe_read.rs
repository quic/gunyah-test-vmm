// Copyright (c) 2024, Qualcomm Innovation Center, Inc. All rights reserved.
// SPDX-License-Identifier: BSD-3-Clause-Clear

use std::sync::Mutex;

use anyhow::Result;
use libc::c_void;

include!(concat!(env!("OUT_DIR"), "/unsafe_read_bindings.rs"));

static LOCK: Mutex<()> = Mutex::<()>::new(());

fn cautious_memcpy_ptr(dst: *mut c_void, size: u64, src: *const c_void) -> Result<(), ()> {
    let lock = LOCK.lock().expect("Failed to lock");
    let ret = unsafe { unsafe_memcpy(src, size, dst) };
    drop(lock);
    if ret == 0 {
        Ok(())
    } else {
        Err(())
    }
}

pub fn cautious_memcpy(dst: &mut [u8], src: &[u8]) -> Result<(), ()> {
    let size = src.len().try_into().unwrap();
    cautious_memcpy_ptr(
        dst.as_mut_ptr() as *mut c_void,
        size,
        src.as_ptr() as *const c_void,
    )
}

#[cfg(test)]
mod tests {
    use std::ptr;

    use claim::assert_err;
    use libc::c_void;

    use crate::unsafe_read::cautious_memcpy_ptr;

    use super::{cautious_memcpy, unsafe_memcpy};

    #[test]
    pub fn test1() {
        let mut buf = vec![0xff; 4];
        cautious_memcpy(&mut buf, b"abcd").expect("failed to copy");
        assert_eq!(buf, [b'a', b'b', b'c', b'd']);
    }

    #[test]
    pub fn test2() {
        let mut buf = vec![0xff; 4];
        let res = cautious_memcpy_ptr(buf.as_mut_ptr() as *mut c_void, 4, ptr::null());
        assert_err!(res);
    }
}
