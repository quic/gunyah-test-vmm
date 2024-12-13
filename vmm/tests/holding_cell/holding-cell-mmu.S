// © 2024 Qualcomm Technologies, Inc. and/or its subsidiaries.
// All Rights Reserved.
//
// SPDX-License-Identifier: BSD-3-Clause
// Changes from Qualcomm Innovation Center, Inc. are provided under the following license:
// Copyright (c) 2024, Qualcomm Innovation Center, Inc. All rights reserved.
// SPDX-License-Identifier: BSD-3-Clause-Clear

// Assume 36-bit stage-2 address space
#define CRT_ADDRSPACE_BITS 36

#define TCR_EL1_T0SZ_SHIFT 0
#define TCR_EL1_IRGN0_SHIFT 8
#define TCR_EL1_ORGN0_SHIFT 10
#define TCR_EL1_SH0_SHIFT 12
#define TCR_EL1_TG0_SHIFT 14
#define TCR_EL1_A1_SHIFT 22
#define TCR_EL1_EPD1_SHIFT 23
#define TCR_EL1_IPS_SHIFT 32

#define TCR_T0SZ (64 - CRT_ADDRSPACE_BITS)
#define TCR_RGN_WB_RWA 1
#define TCR_SH_INNER 3
#define TCR_TG_4KB 0
#define TCR_IPS_36BITS 1

#define TCR_EL1_CRT ( \
	(TCR_T0SZ << TCR_EL1_T0SZ_SHIFT) | \
	(TCR_RGN_WB_RWA << TCR_EL1_IRGN0_SHIFT) | \
	(TCR_RGN_WB_RWA << TCR_EL1_ORGN0_SHIFT) | \
	(TCR_SH_INNER << TCR_EL1_SH0_SHIFT) | \
	(TCR_TG_4KB << TCR_EL1_TG0_SHIFT) | \
	(0 << TCR_EL1_A1_SHIFT) | \
	(1 << TCR_EL1_EPD1_SHIFT) | \
	(TCR_IPS_36BITS << TCR_EL1_IPS_SHIFT))

#define MAIR_DEFAULTS	0x0004080cbb4fff44
// 63:56 - Attr7 - 0x00 - Device-nGnRnE
// 55:48 - Attr6 - 0x04 - Device-nGnRE
// 47:40 - Attr5 - 0x08 - Device-nGRE
// 39:32 - Attr4 - 0x0c - Device-GRE
// 31:24 - Attr3 - 0xBB - Normal inner/outer WT/RA/WA
// 23:16 - Attr2 - 0x4F - Outer NC, Inner WB/RA/WA
// 15:8  - Attr1 - 0xFF - Normal inner/outer WB/RA/WA
// 7:0   - Attr0 - 0x44 - Normal inner/outer non-cachable.
#define MAIR_ATTRIDX_NORMAL 1

#define VMSAv8_ADDRESS_BITS_LEVEL1 30
#define VMSAv8_ENTRY_BITS 3

#define CRT_PT_LEVEL1_ALIGN (CRT_ADDRSPACE_BITS - 27)
#define CRT_PT_LEVEL1_SIZE (1 << CRT_PT_LEVEL1_ALIGN)
#define CRT_PT_LEVEL1_ENTRIES (1 << (CRT_ADDRSPACE_BITS - 30))

#define VMSAv8_ATTR_IDX_SHIFT 2
#define VMSAv8_AP_SHIFT 6
#define VMSAv8_SH_SHIFT 8
#define VMSAv8_AF_SHIFT 10
#define VMSAv8_CONTIG_SHIFT 52

#define VMSAv8_BLOCK_TYPE 1
#define VMSAv8_AP_RW_EL0_NONE 0
#define VMSAv8_SH_INNER 3

#define VMSAv8_BLOCK_ATTRS_CRT ( \
	VMSAv8_BLOCK_TYPE | \
	(MAIR_ATTRIDX_NORMAL << VMSAv8_ATTR_IDX_SHIFT) | \
	(VMSAv8_AP_RW_EL0_NONE << VMSAv8_AP_SHIFT) | \
	(VMSAv8_SH_INNER << VMSAv8_SH_SHIFT) | \
	(1 << VMSAv8_AF_SHIFT) | \
	(1 << VMSAv8_CONTIG_SHIFT))

#define TTBR_CNP_MASK 1

#define SCTLR_EL1_M_SHIFT 0
#define SCTLR_EL1_C_SHIFT 2
#define SCTLR_EL1_SA_SHIFT 3
#define SCTLR_EL1_I_SHIFT 12

#define SCTLR_MMU_ENABLE_MASK ( \
	(1 << SCTLR_EL1_M_SHIFT) | \
	(1 << SCTLR_EL1_C_SHIFT) | \
	(1 << SCTLR_EL1_SA_SHIFT) | \
	(1 << SCTLR_EL1_I_SHIFT))

.macro	abs64 reg:req val64:req
	movz	\reg, (\val64) & 0xffff
	.ifne ((\val64) >> 16) & 0xffff
	movk	\reg, ((\val64) >> 16) & 0xffff, LSL 16
	.endif
	.ifne ((\val64) >> 32) & 0xffff
	movk	\reg, ((\val64) >> 32) & 0xffff, LSL 32
	.endif
	.ifne ((\val64) >> 48) & 0xffff
	movk	\reg, ((\val64) >> 48) & 0xffff, LSL 48
	.endif
.endm

.macro adrl reg:req sym:req
	adrp	\reg, \sym
	add	\reg, \reg, :lo12:\sym
.endm

.global construct_page_table
.section .text, "ax", @progbits
.balign 8
construct_page_table:
	// Construct page table of 1:1 mappings with 1GB blocks
	adrl	x0, aarch64_crt_ttbr0_level1
	abs64	x1, VMSAv8_BLOCK_ATTRS_CRT
	mov	x2, xzr
__write_pte:
	orr	x3, x1, x2, lsl VMSAv8_ADDRESS_BITS_LEVEL1
	str	x3, [x0, x2, lsl VMSAv8_ENTRY_BITS]
	add	x2, x2, 1
	cmp	x2, CRT_PT_LEVEL1_ENTRIES
	b.ne	__write_pte
.size construct_page_table, . - construct_page_table


.global enable_mmu
.section .text, "ax", @progbits
.balign 8
enable_mmu:
	// Set TCR_EL1
	abs64	x0, TCR_EL1_CRT
	msr	TCR_EL1, x0

	// Set MAIR_EL1
	abs64	x0, MAIR_DEFAULTS
	msr	MAIR_EL1, x0

	// Set TTBR0_EL1
	adrl	x0, aarch64_crt_ttbr0_level1
	orr	x0, x0, TTBR_CNP_MASK
	msr	TTBR0_EL1, x0

	// Flush TLB plus outstanding stores & sysreg writes
	tlbi	vmalle1
	dsb	nsh
	isb

	// Enable MMU via SCTLR_EL1
	mrs	x0, SCTLR_EL1
	abs64	x1, SCTLR_MMU_ENABLE_MASK
	orr	x0, x0, x1
	msr	SCTLR_EL1, x0
	isb

	ret
.size enable_mmu, . - enable_mmu

	.section .bss.page_table, "aw", @nobits
	.p2align CRT_PT_LEVEL1_ALIGN
	.global aarch64_crt_ttbr0_level1
aarch64_crt_ttbr0_level1:
	.space CRT_PT_LEVEL1_SIZE
