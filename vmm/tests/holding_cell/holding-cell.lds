ENTRY(start);

SECTIONS
{
	. = 0x80000000;
	.start : { *(.start) }

	.text : { *(.text) }
	.rodata : { *(.rodata) }
	.data : { *(.data) }
	.data.rel.ro : { *(.data.rel.ro) }
	.got : { *(.got) }
	.bss : { *(.bss) }

	. = ALIGN(4096);

	dtb = .;

	. = . + 1;
	. = ALIGN(4096);

	stack = .;
}
