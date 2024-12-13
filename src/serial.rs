// Copyright (c) 2024, Qualcomm Innovation Center, Inc. All rights reserved.
// SPDX-License-Identifier: BSD-3-Clause-Clear

use std::fmt::Debug;
use std::sync::Mutex;
use std::thread;
use std::{io::Write, ops::Deref, sync::Arc};

use anyhow::{anyhow, Context, Result};
use derive_more::Constructor;
use vm_superio::{serial::NoEvents, Serial, Trigger};
use vmm::{BusDevice, FdtWriter, GunyahInterrupt, GunyahVirtualMachine};

const SERIAL_MMIO_SIZE: u64 = 8;

#[derive(Constructor, Debug)]
struct GunyahEventTrigger(Arc<GunyahInterrupt>);
impl Trigger for GunyahEventTrigger {
    type E = anyhow::Error;

    fn trigger(&self) -> Result<(), Self::E> {
        self.0.trigger().context("Failed to trigger event")
    }
}

impl Deref for GunyahEventTrigger {
    type Target = Arc<GunyahInterrupt>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug)]
pub struct SerialDevice<W: Write + Debug + Send> {
    serial: Serial<GunyahEventTrigger, NoEvents, W>,
    start: u64,
}

impl<W: Write + Debug + 'static + Send> SerialDevice<W> {
    pub fn new(
        vm: &mut GunyahVirtualMachine,
        start: u64,
        interrupt_line: u32,
        out: W,
    ) -> Result<Arc<Mutex<Self>>> {
        let device = Arc::new(Mutex::new(Self {
            serial: Serial::new(
                GunyahEventTrigger::new(vm.add_edge_interrupt(interrupt_line)?),
                out,
            ),
            start,
        }));

        vm.add_device(device.clone(), start, start + SERIAL_MMIO_SIZE)?;

        let stdin_serial = device.clone();
        thread::spawn(move || loop {
            let mut buf = String::new();
            let ret = std::io::stdin().read_line(&mut buf).unwrap();
            if ret > 0 {
                let mut stdin = stdin_serial.lock().unwrap();
                if stdin.serial.fifo_capacity() >= ret {
                    stdin.serial.enqueue_raw_bytes(buf.as_bytes()).unwrap();
                }
            }
        });
        Ok(device)
    }

    pub fn device_name(&self) -> String {
        format!("serial@{:x}", self.start)
    }
}

impl<W: Write + Debug + 'static + Send> BusDevice for SerialDevice<W> {
    fn debug_label(&self) -> String {
        "ns16550a serial".to_string()
    }

    fn read(&mut self, offset: vmm::BusAccessInfo, data: &mut [u8]) -> Result<()> {
        if data.len() != 1 {
            return Err(anyhow!("Only reads of size 1 allowed"));
        }

        data[0] = self.serial.read(offset.offset.try_into().unwrap());
        Ok(())
    }

    fn write(&mut self, offset: vmm::BusAccessInfo, data: &[u8]) -> Result<()> {
        if data.len() != 1 {
            return Err(anyhow!("Only writes of size 1 allowed"));
        }
        self.serial
            .write(offset.offset.try_into().unwrap(), data[0])
            .map_err(|e| {
                anyhow!(format!(
                    "Failed to write to offset: {:x}: {:?}",
                    offset.offset, e
                ))
            })
    }

    fn device_config(&self, fdt: &mut FdtWriter) -> anyhow::Result<()> {
        let node = fdt.begin_node(&self.device_name())?;
        fdt.property_string_list("compatible", vec!["ns16550a".to_string()])?;
        fdt.property_array_u64("reg", vec![self.start, SERIAL_MMIO_SIZE].as_slice())?;
        let irq_config = self.serial.interrupt_evt().fdt_config();
        fdt.property_array_u32("interrupts", &irq_config)?;
        fdt.property_u32("clock-frequency", 0x1C2000)?;
        fdt.end_node(node)?;
        Ok(())
    }
}
