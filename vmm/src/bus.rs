// Copyright (c) 2024, Qualcomm Innovation Center, Inc. All rights reserved.
// SPDX-License-Identifier: BSD-3-Clause-Clear

use std::fmt::Debug;
use std::{
    cmp::Ordering,
    collections::BTreeMap,
    fmt::Display,
    result,
    sync::{Arc, Mutex},
};

use anyhow::{anyhow, Context};
use thiserror::Error as ThisError;
pub use vm_fdt::FdtWriter;

#[derive(ThisError, Debug)]
pub enum Error {
    #[error("Bus Range not found")]
    Empty,
    /// The insertion failed because the new device overlapped with an old device.
    #[error("new device {base},{len} overlaps with an old device {other_base},{other_len}")]
    Overlap {
        base: u64,
        len: u64,
        other_base: u64,
        other_len: u64,
    },
}

pub type Result<T> = result::Result<T, Error>;

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum AccessId {
    VmmUserspace,
    Vcpu(u8),
}

/// Information about how a device was accessed.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct BusAccessInfo {
    /// Offset from base address that the device was accessed at.
    pub offset: u64,
    /// Absolute address of the device's access in its address space.
    pub address: u64,
    /// ID of the entity requesting a device access, usually the VCPU id.
    pub id: AccessId,
}

pub trait BusDevice: Send {
    fn debug_label(&self) -> String;
    /// Reads at `offset` from this device
    fn read(&mut self, _offset: BusAccessInfo, _data: &mut [u8]) -> anyhow::Result<()> {
        Err(anyhow!("Unhandled read"))
    }
    /// Writes at `offset` into this device
    fn write(&mut self, _offset: BusAccessInfo, _data: &[u8]) -> anyhow::Result<()> {
        Err(anyhow!("Unhandled write"))
    }

    fn memory_regions(&self) -> Option<Box<[u64]>> {
        None
    }
    fn gunyah_vdevice_config(&self, _fdt: &mut FdtWriter) -> anyhow::Result<()> {
        Ok(())
    }
    fn device_config(&self, _fdt: &mut FdtWriter) -> anyhow::Result<()> {
        Ok(())
    }
}

pub trait BusDeviceSync: BusDevice + Sync {
    fn read(&self, offset: BusAccessInfo, data: &mut [u8]) -> anyhow::Result<()>;
    fn write(&self, offset: BusAccessInfo, data: &[u8]) -> anyhow::Result<()>;
}

/// Holds a base and length representing the address space occupied by a `BusDevice`.
///
/// * base - The address at which the range start.
/// * len - The length of the range in bytes.
#[derive(Copy, Clone)]
pub struct BusRange {
    pub base: u64,
    pub len: u64,
}

impl BusRange {
    /// Returns true if `addr` is within the range.
    pub fn contains(&self, addr: u64) -> bool {
        self.base <= addr && addr < self.base.saturating_add(self.len)
    }

    /// Returns true if there is overlap with the given range.
    pub fn overlaps(&self, base: u64, len: u64) -> bool {
        self.base < base.saturating_add(len) && base < self.base.saturating_add(self.len)
    }
}

impl Eq for BusRange {}

impl PartialEq for BusRange {
    fn eq(&self, other: &BusRange) -> bool {
        self.base == other.base
    }
}

impl Ord for BusRange {
    fn cmp(&self, other: &BusRange) -> Ordering {
        self.base.cmp(&other.base)
    }
}

impl PartialOrd for BusRange {
    fn partial_cmp(&self, other: &BusRange) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl std::fmt::Debug for BusRange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:#x}..+{:#x}", self.base, self.len)
    }
}

#[derive(Clone, Debug)]
struct BusEntry {
    device: BusDeviceEntry,
}

#[derive(Clone)]
enum BusDeviceEntry {
    OuterSync(Arc<Mutex<dyn BusDevice>>),
    InnerSync(Arc<dyn BusDeviceSync>),
}

impl Debug for BusDeviceEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OuterSync(arg0) => f
                .debug_tuple("OuterSync")
                .field(&arg0.lock().unwrap().debug_label())
                .finish(),
            Self::InnerSync(arg0) => f
                .debug_tuple("InnerSync")
                .field(&arg0.debug_label())
                .finish(),
        }
    }
}

impl Display for BusDeviceEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BusDeviceEntry::OuterSync(arg) => f.write_str(&arg.lock().unwrap().debug_label()),
            BusDeviceEntry::InnerSync(arg) => f.write_str(&arg.debug_label()),
        }
    }
}

/// A device container for routing reads and writes over some address space.
///
/// This doesn't have any restrictions on what kind of device or address space this applies to. The
/// only restriction is that no two devices can overlap in this address space.
#[derive(Clone, Debug)]
pub struct Bus {
    devices: Arc<Mutex<BTreeMap<BusRange, BusEntry>>>,
    access_id: AccessId,
}

impl Display for Bus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Bus")
            .field(&self.access_id)
            .field(&self.devices.lock().unwrap())
            .finish()
    }
}

impl Default for Bus {
    fn default() -> Self {
        Self::new()
    }
}

impl Bus {
    /// Constructs an a bus with an empty address space.
    pub fn new() -> Bus {
        Bus {
            devices: Arc::new(Mutex::new(BTreeMap::new())),
            access_id: AccessId::VmmUserspace,
        }
    }

    /// Sets the id that will be used for BusAccessInfo.
    pub fn set_access_id(&mut self, id: AccessId) -> Self {
        let mut bus = self.clone();
        bus.access_id = id;
        bus
    }

    fn first_before(&self, addr: u64) -> Option<(BusRange, BusEntry)> {
        let devices = self.devices.lock().unwrap();
        let (range, entry) = devices
            .range(..=BusRange { base: addr, len: 1 })
            .next_back()?;
        Some((*range, entry.clone()))
    }

    fn get_device(&self, addr: u64) -> Option<(u64, u64, BusEntry)> {
        if let Some((range, entry)) = self.first_before(addr) {
            let offset = addr - range.base;
            if offset < range.len {
                return Some((offset, addr, entry));
            }
        }
        None
    }

    /// Puts the given device at the given address space.
    pub fn insert(&self, device: Arc<Mutex<dyn BusDevice>>, base: u64, len: u64) -> Result<()> {
        if len == 0 {
            return Err(Error::Overlap {
                base,
                len,
                other_base: 0,
                other_len: 0,
            });
        }

        // Reject all cases where the new device's range overlaps with an existing device.
        let mut devices = self.devices.lock().unwrap();
        devices.iter().try_for_each(|(range, _dev)| {
            if range.overlaps(base, len) {
                Err(Error::Overlap {
                    base,
                    len,
                    other_base: range.base,
                    other_len: range.len,
                })
            } else {
                Ok(())
            }
        })?;
        if devices
            .insert(
                BusRange { base, len },
                BusEntry {
                    device: BusDeviceEntry::OuterSync(device),
                },
            )
            .is_some()
        {
            return Err(Error::Overlap {
                base,
                len,
                other_base: base,
                other_len: len,
            });
        }

        Ok(())
    }

    /// Puts the given device that implements BusDeviceSync at the given address space. Devices
    /// that implement BusDeviceSync manage thread safety internally, and thus can be written to
    /// by multiple threads simultaneously.
    pub fn insert_sync(&self, device: Arc<dyn BusDeviceSync>, base: u64, len: u64) -> Result<()> {
        if len == 0 {
            return Err(Error::Overlap {
                base,
                len,
                other_base: 0,
                other_len: 0,
            });
        }

        // Reject all cases where the new device's range overlaps with an existing device.
        let mut devices = self.devices.lock().unwrap();
        devices.iter().try_for_each(|(range, _dev)| {
            if range.overlaps(base, len) {
                Err(Error::Overlap {
                    base,
                    len,
                    other_base: range.base,
                    other_len: range.len,
                })
            } else {
                Ok(())
            }
        })?;

        if devices
            .insert(
                BusRange { base, len },
                BusEntry {
                    device: BusDeviceEntry::InnerSync(device),
                },
            )
            .is_some()
        {
            return Err(Error::Overlap {
                base,
                len,
                other_base: base,
                other_len: len,
            });
        }

        Ok(())
    }

    /// Remove the given device at the given address space.
    pub fn remove(&self, base: u64, len: u64) -> Result<()> {
        if len == 0 {
            return Err(Error::Overlap {
                base,
                len,
                other_base: 0,
                other_len: 0,
            });
        }

        let mut devices = self.devices.lock().unwrap();
        if devices
            .iter()
            .any(|(range, _dev)| range.base == base && range.len == len)
        {
            let ret = devices.remove(&BusRange { base, len });
            if ret.is_some() {
                Ok(())
            } else {
                Err(Error::Empty)
            }
        } else {
            Err(Error::Empty)
        }
    }

    /// Reads data from the device that owns the range containing `addr` and puts it into `data`.
    ///
    /// Returns true on success, otherwise `data` is untouched.
    pub fn read(&self, addr: u64, data: &mut [u8]) -> anyhow::Result<()> {
        if let Some((offset, address, entry)) = self.get_device(addr) {
            let io = BusAccessInfo {
                address,
                offset,
                id: self.access_id,
            };

            match &entry.device {
                BusDeviceEntry::OuterSync(dev) => {
                    let mut device = dev.lock().unwrap();
                    device
                        .read(io, data)
                        .context(format!("{} failed to handle read", device.debug_label()))
                }
                BusDeviceEntry::InnerSync(dev) => dev
                    .read(io, data)
                    .context(format!("{} failed to handle read", dev.debug_label())),
            }
        } else {
            Err(anyhow!("No device suitable"))
        }
    }

    /// Writes `data` to the device that owns the range containing `addr`.
    ///
    /// Returns true on success, otherwise `data` is untouched.
    pub fn write(&self, addr: u64, data: &[u8]) -> anyhow::Result<()> {
        if let Some((offset, address, entry)) = self.get_device(addr) {
            let io = BusAccessInfo {
                address,
                offset,
                id: self.access_id,
            };

            match &entry.device {
                BusDeviceEntry::OuterSync(dev) => {
                    let mut device = dev.lock().unwrap();
                    device
                        .write(io, data)
                        .context(format!("{} failed to handle write", device.debug_label()))
                }
                BusDeviceEntry::InnerSync(dev) => dev
                    .write(io, data)
                    .context(format!("{} failed to handle write", dev.debug_label())),
            }
        } else {
            Err(anyhow!("No device suitable"))
        }
    }

    pub fn generate_gunyah_vdevice_config(&self, fdt: &mut FdtWriter) -> anyhow::Result<()> {
        let devices = self.devices.lock().unwrap();
        devices
            .iter()
            .try_for_each(|(_range, device)| match &device.device {
                BusDeviceEntry::OuterSync(dev) => dev.lock().unwrap().gunyah_vdevice_config(fdt),
                BusDeviceEntry::InnerSync(dev) => dev.gunyah_vdevice_config(fdt),
            })
    }

    pub fn list_memory_regions(&self) -> Vec<u64> {
        let mut vec = Vec::<u64>::new();
        let devices = self.devices.lock().unwrap();
        devices
            .iter()
            .for_each(|(_range, device)| match &device.device {
                BusDeviceEntry::OuterSync(dev) => {
                    let device = dev.lock().unwrap();
                    if let Some(regions) = device.memory_regions() {
                        vec.extend_from_slice(&regions);
                    }
                }
                BusDeviceEntry::InnerSync(dev) => {
                    if let Some(regions) = dev.memory_regions() {
                        vec.extend_from_slice(&regions);
                    }
                }
            });
        vec
    }

    pub fn generate_device_config(&self, fdt: &mut FdtWriter) -> anyhow::Result<()> {
        let devices = self.devices.lock().unwrap();
        devices
            .iter()
            .try_for_each(|(_range, device)| match &device.device {
                BusDeviceEntry::OuterSync(dev) => dev.lock().unwrap().device_config(fdt),
                BusDeviceEntry::InnerSync(dev) => dev.device_config(fdt),
            })
    }
}
