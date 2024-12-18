// Copyright (c) 2024, Qualcomm Innovation Center, Inc. All rights reserved.
// SPDX-License-Identifier: BSD-3-Clause-Clear

use std::{
    env,
    io::stderr,
    path::{Path, PathBuf},
    process::Command,
};

const HOLDING_CELL_SOURCES: [&str; 3] = [
    "tests/holding_cell/holding-cell.c",
    "tests/holding_cell/holding-cell-mmu.S",
    "tests/holding_cell/holding-cell-vtable.S",
];

fn build_unsafe_read(out_dir: &Path) {
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

fn build_holding_cell(out_dir: &Path) {
    for source in HOLDING_CELL_SOURCES {
        println!("cargo:rerun-if-changed={}", source);
    }
    println!("cargo:rerun-if-changed=tests/holding_cell/holding-cell.lds");

    let compiler = cc::Build::new().get_compiler();

    let elf_path = out_dir.join("holding-cell.elf");
    let elf_str = elf_path.to_str().unwrap();

    let bin_path = out_dir.join("holding-cell.bin");
    let bin_str = bin_path.to_str().unwrap();

    assert!(Command::new(compiler.path())
        .args(["-o", elf_str])
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
            "-fno-unwind-tables",
        ])
        .stderr(stderr())
        .status()
        .unwrap()
        .success());

    let objcopy = cargo_binutils::Tool::Objcopy.path().unwrap();
    assert!(Command::new(objcopy)
        .args(["-O", "binary"])
        .arg(elf_str)
        .arg(bin_str)
        .stderr(stderr())
        .status()
        .unwrap()
        .success());
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    build_unsafe_read(&out_dir);
    build_holding_cell(&out_dir);
}
