// Copyright (c) 2024, Qualcomm Innovation Center, Inc. All rights reserved.
// SPDX-License-Identifier: BSD-3-Clause-Clear

#define HOLDING_CELL_ADDR	(void *)(0x6000)
#define HOLDING_CELL_ERR_ADDR	(void *)(0x6008)
#define HOLDING_CELL_EXCEPTION_ADDR	(void *)(0x7000)

#define report_err()		do { *(unsigned long*)HOLDING_CELL_ERR_ADDR = __LINE__; } while(0)

#define COMMAND_HOLD(cmd)	(!!((cmd) & 0x1000))
#define COMMAND_NARGS(cmd)	(((cmd) >> 8) & 0xf)
#define COMMAND_ID(cmd)		((cmd) & 0xff)

struct command {
	unsigned long nargs;
	union {
		long (*cb0)(void);
		long (*cb1)(unsigned long);
		long (*cb2)(unsigned long, unsigned long);
		long (*cb5)(unsigned long, unsigned long, unsigned long, unsigned long, unsigned long);
	};
};

long test_ok(void) {
	return 0;
}

long test_nok(void) {
	return -1;
}

long read_addr(unsigned long addr) {
	return *(volatile unsigned long *)addr;
}

long write_addr(unsigned long addr, unsigned long value) {
	*(volatile unsigned long *)addr = value;
	return 0;
}

long test_lo(unsigned long value) {
	return value;
}

long test_magic(unsigned long value) {
	return value == 0xdeadf00d;
}

long smccc_hvc(unsigned long fnid, unsigned long arg1, unsigned long arg2,
	   unsigned long arg3, unsigned long arg4)
{
	register unsigned long r0 asm("r0");
	register unsigned long a0 asm("r0") = fnid;
	register unsigned long a1 asm("r1") = arg1;
	register unsigned long a2 asm("r2") = arg2;
	register unsigned long a3 asm("r3") = arg3;
	register unsigned long a4 asm("r4") = arg4;

	asm volatile (
		"hvc #0\n" :
		"=r" (r0)
		: "r" (a0), "r" (a1), "r" (a2), "r" (a3), "r" (a4)
		: "x16", "x30", "cc", "memory"
	);

	return r0;
}

long access_page_range(unsigned long start, unsigned long length)
{
	for (unsigned long addr = start; addr < start + length; addr += 4096) {
		write_addr(addr, 0xa5a5a5a5);
	}
	return 0;
}

long read_io(unsigned long addr) {
	return *(volatile unsigned long *)addr;
}

long write_io(unsigned long addr, unsigned long value) {
	*(volatile unsigned long *)addr = value;
	return 0;
}

#define TEST(sym, _nargs, id) \
	[id] = { .nargs = _nargs, .cb ## _nargs = sym }

#define NR_COMMANDS		11
const struct command COMMANDS[NR_COMMANDS]= {
	/*   function         nargs    id */
	TEST(test_ok,		0,	0),
	TEST(test_nok,		0,	1),
	TEST(read_addr,		1,	2),
	TEST(write_addr,	2,	3),
	TEST(test_lo,		1,	4),
	TEST(test_magic,	1,	5),
	TEST(smccc_hvc,		5,	6),
	TEST(access_page_range,	2,	7),
	TEST(read_io,		1,	8),
	TEST(write_io,		2,	9),
};

int main() {
	volatile unsigned long * const holding_cell = HOLDING_CELL_ADDR;
	unsigned long word, nargs, command, i, tmp;
	const struct command *cmd;

	while(1) {
		word = *holding_cell;
		command = COMMAND_ID(word);
		if (command > NR_COMMANDS || !(cmd = &COMMANDS[command])) {
			report_err();
			continue;
		}

		nargs = COMMAND_NARGS(word);
		if (nargs > cmd->nargs) {
			while (nargs--)
				tmp = *holding_cell;
			report_err();
			continue;
		}

		unsigned long args[nargs];

		for (i = 0; i < nargs; i++)
			args[i] = *holding_cell;

		if (COMMAND_HOLD(word))
			tmp = *holding_cell;

		(void)tmp;

		switch (nargs) {
		case 0:
			*holding_cell = cmd->cb0();
			break;
		case 1:
			*holding_cell = cmd->cb1(args[0]);
			break;
		case 2:
			*holding_cell = cmd->cb2(args[0], args[1]);
			break;
		case 5:
			*holding_cell = cmd->cb5(args[0], args[1], args[2], args[3], args[4]);
		}
	}
	__builtin_unreachable();
}

extern void construct_page_table();
extern void enable_mmu();

void __attribute__ ((noinline)) __start()
{
	unsigned long mpidr;

	asm (
		"mrs %[mpidr], MPIDR_EL1\n"
		: [mpidr] "=r" (mpidr)
	);

	if ((mpidr & 0xff) == 0)
		construct_page_table();

	enable_mmu();

	main();
}

extern void *stack;
extern void *vector_table;

void __attribute__((section(".start"))) start() {
	asm (
		"mrs x0, MPIDR_EL1\n"
		"and x0, x0, #0xff\n"
		"add x0, x0, #1\n"
		"lsl x0, x0, #12\n"
		"add x0, x0, %[stack_start]\n"
		"mov sp, x0\n"
		:
		: [stack_start] "r" (&stack)
		: "cc", "x0"
	);

	asm (
		"mrs x0, SCTLR_EL1\n"
		"orr x0, x0, #4\n" // Enable C bit
		"msr SCTLR_EL1, x0\n"
		"msr VBAR_EL1, %[vbar]\n"
		"isb\n"
		:
		: [vbar] "r" (&vector_table)
		: "x0"
	);

	__start();
}

void sync_abort() {
	unsigned long esr, far;
	volatile unsigned long *catch = HOLDING_CELL_EXCEPTION_ADDR;

	asm (
		"mrs %[esr], ESR_EL1\n"
		: [esr] "=r" (esr)
	);
	*catch = esr;

	asm (
		"mrs %[far], FAR_EL1\n"
		: [far] "=r" (far)
	);
	*catch = far;

	while (1) ;
}
