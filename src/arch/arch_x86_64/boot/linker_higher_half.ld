ENTRY(start)
OUTPUT_FORMAT(elf64-x86-64)

/*  this offset CANNOT be any smaller than 2GiB below the end of the 64-bit address space
 *  because it must fit within 32-bit address space, 
 *  otherwise you'll get horribly misleading linker errors.... 
 *  ask me how I know...
 */
KERNEL_BASE = 0xFFFFFFFF80000000;

SECTIONS {

	. = 1M;

	.init ALIGN(4K) :
	{
		KEEP(*(.multiboot_header))
		*(.init .init.*)
	}

	. += KERNEL_BASE;

	.text ALIGN(4K) : AT(ADDR(.text) - KERNEL_BASE)
	{
		*(.text .text.*)
	}

	.data ALIGN(4K) : AT(ADDR(.data) - KERNEL_BASE)
	{
		*(.padata)
		*(.data .data.* )
	}

	.bss ALIGN(4K) : AT(ADDR(.bss) - KERNEL_BASE)
	{
		*(.bss .bss.*)
	}

	.rodata ALIGN(4K) : AT(ADDR(.rodata) - KERNEL_BASE)
	{
		*(.rodata .rodata.*)
	}

	.got ALIGN(4K) : AT(ADDR(.got) - KERNEL_BASE)
	{
		*(.got)
	}

	.got.plt ALIGN(4K) : AT(ADDR(.got.plt) - KERNEL_BASE)
	{
		*(.got.plt)
	}

	.data.rel.ro ALIGN(4K) : AT(ADDR(.data.rel.ro) - KERNEL_BASE)
	{
		*(.data.rel.ro.local*) *(.data.rel.ro .data.rel.ro.*)
	}

	.gcc_except_table ALIGN(4K) : AT(ADDR(.gcc_except_table) - KERNEL_BASE)
	{
		*(.gcc_except_table)
	}
}
