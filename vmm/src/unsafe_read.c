// Copyright (c) 2024, Qualcomm Innovation Center, Inc. All rights reserved.
// SPDX-License-Identifier: BSD-3-Clause-Clear

int unsafe_memcpy(const void *addr, unsigned long size, void *dst);

#ifndef __BINDGEN__

#include <signal.h>
#include <setjmp.h>
#include <stdint.h>
#include <string.h>

jmp_buf buf;

void signal_handler(int s)
{
	(void)s;

	longjmp(buf, 1);
}

int unsafe_memcpy(const void *addr, unsigned long size, void *dst)
{
	struct sigaction old_sigsegv, old_sigbus;
	struct sigaction sa = {
		.sa_handler = signal_handler,
	};
	int ret;

	sigemptyset(&sa.sa_mask);

	if(sigaction(SIGSEGV, &sa, &old_sigsegv))
		return -1;

	ret = sigaction(SIGBUS, &sa, &old_sigbus);
	if (ret)
		goto restore_sigsegv;

	ret = setjmp(buf);
	if (ret)
		goto out;

	memcpy(dst, addr, size);
out:
	sigaction(SIGBUS, &old_sigbus, NULL);
restore_sigsegv:
	sigaction(SIGSEGV, &old_sigsegv, NULL);
	return ret;
}

#endif
