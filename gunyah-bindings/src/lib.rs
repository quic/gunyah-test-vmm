// Copyright (c) 2024, Qualcomm Innovation Center, Inc. All rights reserved.
// SPDX-License-Identifier: BSD-3-Clause-Clear

#[macro_use]
extern crate nix;

use std::fmt::Debug;

#[cfg(not(feature = "ack-bindings"))]
pub mod bindings;
#[cfg(not(feature = "ack-bindings"))]
pub use bindings::*;

#[cfg(feature = "ack-bindings")]
pub mod ack_bindings;
#[cfg(feature = "ack-bindings")]
pub use ack_bindings::*;

pub mod ioctls;
pub use ioctls::*;

impl Debug for gunyah_vcpu_run {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut s = f.debug_struct("gunyah_vcpu_run");
        s.field("immediate_exit", &self.immediate_exit)
            .field("exit_reason", &self.exit_reason);

        match self.exit_reason {
            // SAFETY: Safe because we checked that the exit_reason is GUNYAH_VCPU_EXIT_MMIO
            gunyah_vcpu_exit::GUNYAH_VCPU_EXIT_MMIO => s
                .field("mmio", unsafe { &self.__bindgen_anon_1.mmio })
                .finish(),
            // SAFETY: Safe because we checked that the exit_reason is GUNYAH_VCPU_EXIT_STATUS
            gunyah_vcpu_exit::GUNYAH_VCPU_EXIT_STATUS => s
                .field("status", unsafe { &self.__bindgen_anon_1.status })
                .finish(),
            // SAFETY: Safe because we checked that the exit_reason is GUNYAH_VCPU_EXIT_PAGE_FAULT
            gunyah_vcpu_exit::GUNYAH_VCPU_EXIT_PAGE_FAULT => s
                .field("page_fault", unsafe { &self.__bindgen_anon_1.page_fault })
                .finish(),
            gunyah_vcpu_exit::GUNYAH_VCPU_EXIT_UNKNOWN => s.finish(),
            _ => s.finish(),
        }
    }
}
