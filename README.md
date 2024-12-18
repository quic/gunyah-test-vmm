# gunyah-test-vmm

## Quick Start

To use this on an Android device, be sure to check the instructions below
on setting up the NDK. Assuming `adb shell` and `adb push` work, you can run:

```sh
rustup component add llvm-tools

cargo test --workspace -- --test-threads 1
```

## Running on Android

To get started building/running on Android, please make sure you have the
Android NDK set up. If you don't, you can do it for this project by running
./setup-android.sh. If you already have it setup, you should remove the ar and
linker configurations from [target.aarch64-linux-android] in .cargo/config.toml
[here](./.cargo/config.toml#L2-L3).

```sh
rustup target add aarch64-linux-android
rustup component add llvm-tools
./setup-android.sh

# build/test as usual
cargo build
```

## Dev Container

If you don't have/want the rust toolchain installed, you can use ./tools/dev_container.

```sh
./tools/dev_container cargo test
```

## License

SPDX-License-Identifier: BSD-3-Clause-Clear

See [LICENSE](LICENSE) for the full license text.
