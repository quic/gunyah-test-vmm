// Copyright (c) 2024, Qualcomm Innovation Center, Inc. All rights reserved.
// SPDX-License-Identifier: BSD-3-Clause-Clear

use std::{env, io::stderr, path::PathBuf, process::Command};

const HOLDING_CELL_SOURCES: [&str; 3] = [
    "tests/holding_cell/holding-cell.c",
    "tests/holding_cell/holding-cell-mmu.S",
    "tests/holding_cell/holding-cell-vtable.S",
];

fn build_unsafe_read(out_dir: &PathBuf) {
    println!("cargo::rerun-if-changed=src/unsafe_read.c");
    cc::Build::new()
        .file("src/unsafe_read.c")
        .compile("unsafe_read");

    let bindings = bindgen::Builder::default()
        .header("src/unsafe_read.c")
        .clang_arg("-D__BINDGEN__")
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .generate()
        .expect("Failed to generate bindings");

    bindings
        .write_to_file(out_dir.join("unsafe_read_bindings.rs"))
        .expect("Couldn't write bindings!");
}

fn main() {
    for source in HOLDING_CELL_SOURCES {
        println!("cargo:rerun-if-changed={}", source);
    }
    println!("cargo:rerun-if-changed=tests/holding_cell/holding-cell.lds");
    println!("cargo:rerun-if-changed=build.rs");

    let cc = env::var("CC_aarch64-linux-android")
        .or(env::var("CC_aarch64_linux_android"))
        .expect("CC_aarch64-linux-android is unset");
    let objcopy = env::var("OBJCOPY").expect("OBJCOPY is unset");
    let out_dir = env::var("OUT_DIR").unwrap();

    build_unsafe_read(&PathBuf::from(&out_dir));

    assert!(Command::new(cc)
        .args(["-o", &format!("{}/holding-cell.elf", &out_dir)])
        .args(HOLDING_CELL_SOURCES)
        .arg("-Os")
        .arg("-static")
        .arg("-nostdlib")
        .arg("-g")
        .args(["-Wl,-T", "tests/holding_cell/holding-cell.lds"])
        .arg("-Wl,--build-id=none")
        .args([
            "-fomit-frame-pointer",
            "-fno-exceptions",
            "-fno-asynchronous-unwind-tables",
            "-fno-unwind-tables"
        ])
        .stderr(stderr())
        .status()
        .unwrap()
        .success());

    assert!(Command::new(objcopy)
        .args(["-O", "binary"])
        .arg(&format!("{}/holding-cell.elf", &out_dir))
        .arg(&format!("{}/holding-cell.bin", &out_dir))
        .stderr(stderr())
        .status()
        .unwrap()
        .success());
}
