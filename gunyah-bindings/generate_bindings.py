#!/usr/bin/env python3
# Copyright (c) 2024, Qualcomm Innovation Center, Inc. All rights reserved.
# SPDX-License-Identifier: BSD-3-Clause-Clear

import argparse
import datetime
import os.path
from pathlib import Path
import re
import subprocess
import tempfile
from typing import List


def generate_sanitized_headers(kernel: str, output: Path, kbuild_args: List[str]):
    subprocess.run(
        ["make", "headers_install", f"INSTALL_HDR_PATH={output}", *kbuild_args],
        check=True,
        cwd=os.path.abspath(kernel),
    )


def install_bindgen():
    subprocess.run(["cargo", "install", "bindgen-cli"], check=True)


def generate_bindings(kernel: str, output: str, arch: str, kbuild_args: List[str]):
    with tempfile.TemporaryDirectory() as sanitized_headers_dir:
        generate_sanitized_headers(
            kernel, sanitized_headers_dir, [f"ARCH={arch}", *kbuild_args]
        )
        headers = Path(sanitized_headers_dir) / "include"
        bindgen_env = os.environ.copy()

        with open(f"{output}", mode="w") as bindings:
            bindings.write(
                "\n".join(
                    [
                        f"// Copyright (c) {datetime.now().year}, Qualcomm Innovation Center, Inc. All rights reserved.",
                        "// SPDX-License-Identifier: BSD-3-Clause-Clear",
                        "",
                        "#![allow(clippy::missing_safety_doc)]",
                        "#![allow(clippy::upper_case_acronyms)]",
                        "#![allow(non_upper_case_globals)]",
                        "#![allow(non_camel_case_types)]",
                        "#![allow(non_snake_case)]",
                        "#![allow(dead_code)]",
                        "",
                        "",
                    ]
                )
            )

            with tempfile.NamedTemporaryFile(mode="r") as tmp_bindings:
                subprocess.run(
                    [
                        "bindgen",
                        "--no-layout-tests",
                        "--no-doc-comments",
                        "--with-derive-default",
                        "--default-enum-style",
                        "moduleconsts",
                        "--blocklist-item=__kernel.*",
                        "--blocklist-item=__BITS_PER_LONG",
                        "--blocklist-item=__FD_SETSIZE",
                        headers / "linux" / "gunyah.h",
                        "-o",
                        tmp_bindings.name,
                        "--",
                        "-isystem",
                        headers,
                    ],
                    env=bindgen_env,
                )

                tmp_bindings.seek(0)
                for line in tmp_bindings:
                    if re.match(r"^pub type __(u|s|(l|b)e)(8|16|32|64) =", line):
                        continue
                    for pre, post in [
                        ("__u", "u"),
                        ("__s", "i"),
                        ("__le", "Le"),
                        ("__be", "Be"),
                    ]:
                        line = re.sub(rf"{pre}(8|16|32|64)", f"{post}\\1", line)

                    # Workaround until aosp/2370138 makes it to https://github.com/rust-vmm/vmm-sys-util/blob/bc1e0062541dec4bd811238fdb66605a18f819ac/src/linux/ioctl.rs#L27
                    for const in ["GUNYAH_IOCTL_TYPE"]:
                        line = re.sub(
                            rf"pub const {const}: u8 = (\d+)u8;",
                            f"pub const {const}: u32 = \\1;",
                            line,
                        )

                    bindings.write(line)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(
        description="Generate Rust bindings from Gunyah headers from Linux kernel"
    )

    parser.add_argument(
        "kernel",
        type=str,
        help="Linux kernel source path",
    )

    parser.add_argument(
        "--kbuild",
        "-k",
        nargs="+",
        default=["CROSS_COMPILE=aarch64-linux-gnu-"],
        help="Arguments to pass to kbuild",
    )

    # TODO: add multiple architecture support

    parser.add_argument(
        "--out",
        "-o",
        dest="output",
        default="gunyah-bindings/src/bindings.rs",
    )

    args = parser.parse_args()

    install_bindgen()
    generate_bindings(args.kernel, args.output, "arm64", args.kbuild)
