#!/bin/bash
# Copyright (c) 2024, Qualcomm Innovation Center, Inc. All rights reserved.
# SPDX-License-Identifier: BSD-3-Clause-Clear

echo "Setting up toolchain to build Android binaries"

set -x

rustup target add aarch64-linux-android

if [ ! -d android-ndk ] ; then
	rm -rf android-ndk-r25c-linux.zip android-ndk-r25c
	wget https://dl.google.com/android/repository/android-ndk-r25c-linux.zip
	unzip android-ndk-r25c-linux
	android-ndk-r25c/build/tools/make_standalone_toolchain.py --arch arm64 --install-dir android-ndk/arm64
	rm -rf android-ndk-r25c-linux.zip android-ndk-r25c
fi
