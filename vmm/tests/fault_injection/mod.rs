// Copyright (c) 2024, Qualcomm Innovation Center, Inc. All rights reserved.
// SPDX-License-Identifier: BSD-3-Clause-Clear

use std::{
    fs::{self, File},
    io::Write,
};

use anyhow::{Context, Result};
use claim::{assert_err, assert_ok};
use libc::c_long;
use nonzero_ext::nonzero;
use rstest::rstest;
use serial_test::serial;
use vm_fdt::FdtWriter;
use vmm::GunyahVirtualMachine;

pub(crate) fn clear_fault_injection() -> Result<()> {
    let mut f = File::options()
        .write(true)
        .open("/sys/kernel/debug/fail_function/inject")?;
    f.write(b"")?;

    fs::write("/proc/self/make-it-fail", "0")?;
    Ok(())
}

pub(crate) fn fault_inject_function(function: &str, error: c_long) -> Result<()> {
    fs::write("/sys/kernel/debug/fail_function/inject", function)
        .context("Couldn't set injection")?;
    fs::write(
        format!("/sys/kernel/debug/fail_function/{}/retval", function),
        format!("0x{:x}", error),
    )
    .context("Couldn't set retval")?;
    fs::write("/sys/kernel/debug/fail_function/probability", "100")
        .context("Couldn't set probability")?;
    fs::write("/sys/kernel/debug/fail_function/times", "-1").context("Couldn't set probability")?;
    fs::write("/sys/kernel/debug/fail_function/task-filter", "Y")
        .context("Couldn't set probability")?;

    fs::write("/proc/thread-self/make-it-fail", "1")?;
    Ok(())
}

#[test]
#[ignore = "assumed fail_function not available"]
#[serial]
fn can_create_injection() {
    clear_fault_injection().expect("Couldn't clear fault injections");
    fault_inject_function("gunyah_rm_alloc_vmid", -(libc::EINVAL as c_long))
        .expect("Couldn't set up a fault injection");
}

fn generate_fdt(vm: &GunyahVirtualMachine) -> Result<Vec<u8>> {
    let mut fdt = FdtWriter::new()?;
    let root_node = fdt.begin_node("")?;
    vm.create_fdt_basic_config(
        &mut fdt,
        &[0x3FFF0000, 0x10000, 0x3FF00000, 0x20000],
        &[13, 14, 11, 10],
    )?;
    fdt.end_node(root_node)?;
    Ok(fdt.finish()?)
}

fn setup_basic_vm() -> Result<GunyahVirtualMachine> {
    let mut vm = GunyahVirtualMachine::new().context("Failed to create VM")?;
    vm.add_memory(
        0x8000_0000,
        nonzero!(4096_usize),
        gunyah::ShareType::Lend,
        gunyah::GuestMemoryAccess::Rwx,
        false,
    )
    .context("Failed to add memory to VM")?;

    vm.create_vcpu(0).context("Failed to create vcpu")?;
    vm.set_boot_pc(0x8000_0000)
        .context("failed to set boot pc")?;

    vm.set_dtb_config(
        0x8000_0000,
        4096_u64,
        &generate_fdt(&vm).expect("Failed to create fdt"),
    )
    .expect("Failed to set dtb config");

    let mut fdt = FdtWriter::new().context("Failed to create fdt writer")?;
    fdt.begin_node("")?;
    vm.create_fdt_basic_config(
        &mut fdt,
        &[0x3FFF0000, 0x10000, 0x3FF00000, 0x20000],
        &[13, 14, 11, 10],
    )
    .context("Failed to create fdt config")?;

    Ok(vm)
}

#[test]
#[ignore = "assumed fail_function not available"]
#[serial]
fn vm_start_basic() {
    clear_fault_injection().expect("Couldn't clear fault injections");

    let vm = setup_basic_vm().expect("Failed to create VM");
    assert_ok!(vm.start());
}

#[rstest]
#[ignore = "assumed fail_function not available"]
#[serial]
fn vm_start_fails(
    #[values(
        "alloc_vmid",
        "mem_share",
        "vm_configure",
        "vm_set_demand_paging",
        "vm_set_address_layout",
        "vm_init",
        "vm_set_boot_context",
        "get_hyp_resources",
        "vm_start"
    )]
    fun: &str,
) {
    clear_fault_injection().expect("Couldn't clear fault injections");
    fault_inject_function(&format!("gunyah_rm_{}", fun), -(libc::EINVAL as c_long))
        .expect("Couldn't set up a fault injection");

    let vm = setup_basic_vm().expect("Failed to create VM");
    assert_err!(vm.start());
}
