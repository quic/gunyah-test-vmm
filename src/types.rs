// Copyright (c) 2024, Qualcomm Innovation Center, Inc. All rights reserved.
// SPDX-License-Identifier: BSD-3-Clause-Clear

use std::{
    fmt::{Debug, Display},
    num::NonZeroUsize,
    ops::{Add, Sub},
    str::FromStr,
};

use anyhow::{anyhow, Context};
use derive_more::{Constructor, Deref};

#[derive(Clone, Constructor, Copy, Deref, PartialEq, Eq, PartialOrd, Ord)]
pub struct GuestAddress(u64);

impl Display for GuestAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:#08x}", self.0)
    }
}

impl Debug for GuestAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "GuestAddress({})", self)
    }
}

impl FromStr for GuestAddress {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        const RADIX_PREFIXES: [(u32, &str); 3] = [(16, "0x"), (2, "0b"), (10, "")];
        const MUL_SUFFIXES: [(u64, &str); 10] = [
            (1073741824, "gib"),
            (1073741824, "gb"),
            (1073741824, "g"),
            (1048576, "mib"),
            (1048576, "mb"),
            (1048576, "m"),
            (1024, "kib"),
            (1024, "kb"),
            (1024, "k"),
            (1, ""),
        ];

        let s = s.trim().to_lowercase();
        let (radix, s) = RADIX_PREFIXES
            .iter()
            .find(|(_, prefix)| s.starts_with(prefix))
            .map(|(radix, prefix)| (radix, s.strip_prefix(prefix).unwrap()))
            .unwrap();
        let (mul, s) = MUL_SUFFIXES
            .iter()
            .find(|(_, suffix)| s.ends_with(suffix))
            .map(|(mul, suffix)| (mul, s.strip_suffix(suffix).unwrap()))
            .unwrap();
        let s = s.replace(['_', ' '], "");
        let val = u64::from_str_radix(&s, *radix)?;
        Ok(GuestAddress(
            val.checked_mul(*mul)
                .context(format!("{val} * {mul} overflows"))?,
        ))
    }
}

impl From<u64> for GuestAddress {
    fn from(value: u64) -> Self {
        GuestAddress(value)
    }
}

#[derive(Clone, Constructor, Copy, Deref, PartialEq, Eq, PartialOrd, Ord)]
pub struct GuestSize(u64);

impl Display for GuestSize {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        const SUFFIXES: [(u64, u32, &str); 4] = [
            (0x3fffffff, 30, "GiB"),
            (0xfffff, 20, "MiB"),
            (0x3ff, 10, "KiB"),
            (0, 0, ""),
        ];

        let v = SUFFIXES
            .iter()
            .find(|(p, _, _)| (self.0 & *p) == 0)
            .map(|(_, shift, suffix)| (self.0 >> shift, suffix))
            .unwrap();

        write!(f, "{}{}", v.0, v.1)
    }
}

impl Debug for GuestSize {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "GuestSize({})", self)
    }
}

impl FromStr for GuestSize {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        const RADIX_PREFIXES: [(u32, &str); 3] = [(16, "0x"), (2, "0b"), (10, "")];
        const MUL_SUFFIXES: [(u64, &str); 10] = [
            (1073741824, "gib"),
            (1073741824, "gb"),
            (1073741824, "g"),
            (1048576, "mib"),
            (1048576, "mb"),
            (1048576, "m"),
            (1024, "kib"),
            (1024, "kb"),
            (1024, "k"),
            (1, ""),
        ];

        let s = s.trim().to_lowercase();
        let (radix, s) = RADIX_PREFIXES
            .iter()
            .find(|(_, prefix)| s.starts_with(prefix))
            .map(|(radix, prefix)| (radix, s.strip_prefix(prefix).unwrap()))
            .unwrap();
        let (mul, s) = MUL_SUFFIXES
            .iter()
            .find(|(_, suffix)| s.ends_with(suffix))
            .map(|(mul, suffix)| (mul, s.strip_suffix(suffix).unwrap()))
            .unwrap();
        let s = s.replace(['_', ' '], "");
        let val = u64::from_str_radix(&s, *radix)?;
        Ok(GuestSize(
            val.checked_mul(*mul)
                .context(format!("{val} * {mul} overflows"))?,
        ))
    }
}

impl From<u64> for GuestSize {
    fn from(value: u64) -> Self {
        Self::new(value)
    }
}

impl From<usize> for GuestSize {
    fn from(value: usize) -> Self {
        Self::new(value.try_into().unwrap())
    }
}

impl TryFrom<GuestSize> for NonZeroUsize {
    type Error = anyhow::Error;

    fn try_from(value: GuestSize) -> Result<Self, Self::Error> {
        NonZeroUsize::new(value.0 as usize).ok_or(anyhow!("Unexpected zero size"))
    }
}

impl Add<GuestSize> for GuestAddress {
    type Output = GuestAddress;

    fn add(self, rhs: GuestSize) -> Self::Output {
        (self.0 + *rhs).into()
    }
}

impl Sub<GuestSize> for GuestAddress {
    type Output = GuestAddress;

    fn sub(self, rhs: GuestSize) -> Self::Output {
        (self.0 - *rhs).into()
    }
}
