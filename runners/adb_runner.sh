#!/bin/bash
# Copyright (c) 2024, Qualcomm Innovation Center, Inc. All rights reserved.
# SPDX-License-Identifier: BSD-3-Clause-Clear

if [ -z "${ADB_SERVER_SOCKET}" ] &&
	[ -z "$(netstat -aon | grep 5037 | grep LISTEN)" -o -z "$(adb devices | grep -v "List of devices attached")" ] ; then
	echo """
WARNING: ADB doesn't appear to be configured correctly.
WARNING: Make sure "adb devices" has a device set up.
WARNING: You might want to set ADB_SERVER_SOCKET to a remote server.
WARNING: To do that, run \`adb -P <PORT> -a nodaemon server\`. Port is typically 5037.
WARNING: Then run on this host: \`export ADB_SERVER_SOCKET=\"tcp:<HOSTNAME>:<PORT>\"\`.
	""" > /dev/stderr
fi


DEVICE_TMP_DIR=/data/local/tmp/

if [ -n "${CARGO_PKG_NAME}" ] ; then
	if [ -n "${CARGO_PKG_VERSION}" ] ; then
		DEVICE_TMP_DIR="${DEVICE_TMP_DIR}${CARGO_PKG_NAME}-${CARGO_PKG_VERSION}/"
	else
		DEVICE_TMP_DIR="${DEVICE_TMP_DIR}${CARGO_PKG_NAME}/"
	fi
fi

DEVICE_FILE="$DEVICE_TMP_DIR/$(basename $1)"

ENV=()
for e in "RUST_BACKTRACE" ; do
	if [ -n "${!e}" ] ; then
		ENV+=("$e=${!e}")
	fi
done

set -e
[ "${ADB_RUNNER_ROOT}" != "0" ] && adb wait-for-device root
adb wait-for-device shell mkdir -p $DEVICE_TMP_DIR
adb wait-for-device push $1 $DEVICE_FILE
shift
[ -z "${ADB_RUNNER_DO_NOT_DELETE}" ] && trap "adb wait-for-device shell rm $DEVICE_FILE" EXIT
adb wait-for-device shell "${ENV[@]}" $DEVICE_FILE "$@"
