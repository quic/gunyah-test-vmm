// Copyright (c) 2024, Qualcomm Innovation Center, Inc. All rights reserved.
// SPDX-License-Identifier: BSD-3-Clause-Clear

extern crate gunyah_bindings;

pub type Error = nix::errno::Errno;

/// A specialized `Result` type for Gunyah ioctls.
///
/// This typedef is generally used to avoid writing out errno::Error directly and
/// is otherwise a direct mapping to Result.
pub type Result<T> = std::result::Result<T, Error>;

pub mod gunyah;
pub use gunyah::*;

pub mod guest_mem;
pub use guest_mem::*;
pub mod vcpu;
pub use vcpu::*;
pub mod vm;
pub use vm::*;
pub mod ioeventfd;
pub use ioeventfd::*;
pub mod irqfd;
pub use irqfd::*;
