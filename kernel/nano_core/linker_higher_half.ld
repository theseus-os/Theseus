ENTRY(_start)
OUTPUT_FORMAT(elf64-x86-64)

/*  this offset CANNOT be any smaller than 2GiB below the end of the 64-bit address space
 *  because it must fit within 32-bit address space, 
 *  otherwise you'll get horribly misleading linker errors.... 
 *  ask me how I know...
 */
KERNEL_OFFSET = 0xFFFFFFFF80000000;

SECTIONS {

	/*
	 * In general, we align each section up to a 4K page boundary in order for Theseus 
	 * to be able to map them to separately-owned pages.
	 */
	. = 1M;

	.init ALIGN(4K) :
	{
		KEEP(*(.multiboot_header))
		*(.init .init.*)
	}

	. += KERNEL_OFFSET;
	
	.text ALIGN(4K) : AT(ADDR(.text) - KERNEL_OFFSET)
	{
		KEEP(*(.pre_init_array))
		*(.text .text.*)
	}

	.rodata ALIGN(4K) : AT(ADDR(.rodata) - KERNEL_OFFSET)
	{
		*(.rodata .rodata.*)
	}

	.eh_frame ALIGN(4K) : AT(ADDR(.eh_frame) - KERNEL_OFFSET)
	{
		*(.eh_frame)
	}

	.gcc_except_table ALIGN(4K) : AT(ADDR(.gcc_except_table) - KERNEL_OFFSET)
	{
		*(.gcc_except_table .gcc_except_table.*)
	}

	.tdata ALIGN(4K) : AT(ADDR(.tdata) - KERNEL_OFFSET)
	{
		*(.tdata .tdata.* )
	}

	/* Don't align .tbss up to a page boundary; it wastes space because .tdata and .tbss are merged. */
	.tbss : AT(ADDR(.tbss) - KERNEL_OFFSET)
	{
		*(.tbss .tbss.*)
	}

	.data ALIGN(4K) : AT(ADDR(.data) - KERNEL_OFFSET)
	{
		*(.padata)
		*(.data .data.* )
	}

	.bss ALIGN(4K) : AT(ADDR(.bss) - KERNEL_OFFSET)
	{
		*(.bss .bss.*)
	}

	.page_table ALIGN(4K) : AT(ADDR(.page_table) - KERNEL_OFFSET)
	{
		*(.page_table)
	}

	/* 
	 * Theseus's boot code uses 2MiB huge pages for its initial mapping,
	 * so we waste a little bit of address space when setting up stack guard pages
	 * in order to keep things simple with an easily-compatible 2MiB guard page.
	 * If we ever use regular 4KiB pages in the boot code, then we can remove/shrink this.
	 */
	. = ALIGN(2M);
	. = . + 2M; /* make space for a 2MiB stack guard page */
	.stack ALIGN(4K) : AT(ADDR(.stack) - KERNEL_OFFSET)
	{
		*(.stack)
	}

	/* .got (Global Offset Table) section and its relocation types are currently unsupported */
	.got (NOLOAD) :
	{
    	KEEP(*(.got .got.*))
  	}
	/*
	.got ALIGN(4K) : AT(ADDR(.got) - KERNEL_OFFSET)
	{
		*(.got)
	}

	.got.plt ALIGN(4K) : AT(ADDR(.got.plt) - KERNEL_OFFSET)
	{
		*(.got.plt)
	}
	*/

	.data.rel.ro ALIGN(4K) : AT(ADDR(.data.rel.ro) - KERNEL_OFFSET)
	{
		*(.data.rel.ro.local*) *(.data.rel.ro .data.rel.ro.*)
	}
}
