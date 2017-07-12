ENTRY(start)
OUTPUT_FORMAT(elf64-x86-64)

/*  this offset CANNOT be any smaller than 2GiB below the end of the 64-bit address space
 *  because it must fit within 32-bit address space, 
 *  otherwise you'll get horribly misleading linker errors.... 
 *  ask me how I know...
 */
KERNEL_OFFSET = 0xFFFFFFFF80000000;

SECTIONS
{
    /* Begin putting sections at 1 MiB, a conventional place for kernels to be
       loaded at by the bootloader. */
    . = 1M;

    . += SIZEOF_HEADERS;

    .init ALIGN(4K) : AT(ADDR(.init))
    {
        KEEP(*(.multiboot))
        *(.inittext)
    }

    /* We want to move the kernel to a higher address */
    . += KERNEL_OFFSET;

    .text ALIGN(4K) : AT(ADDR(.text) - KERNEL_OFFSET)
    {
       *(.text .text.*)
    }

    /* Read-only data. */
    .rodata ALIGN(4K) 
    : AT(ADDR(.rodata) - KERNEL_OFFSET)
    {
        *(.rodata .rodata.*)
    }

    /* Read-write data (initialized) */
    .data ALIGN(4K) : AT(ADDR(.data) - KERNEL_OFFSET)
    {
        *(.padata .padata.*)
        . = ALIGN(4K);
        *(.data .data.*)
    }

    /* Read-write data (uninitialized) and stack */
    .bss ALIGN(4K) : AT(ADDR(.bss) - KERNEL_OFFSET)
    {
        *(.bss .bss.*)
    }



    .got ALIGN(4K) : AT(ADDR(.got) - KERNEL_OFFSET)
    {
        *(.got)
        . = ALIGN(4K);
    }

    .got.plt ALIGN(4K) : AT(ADDR(.got.plt) - KERNEL_OFFSET)
    {
        *(.got.plt)
        . = ALIGN(4K);
    }

    .data.rel.ro ALIGN(4K) : AT(ADDR(.data.rel.ro) - KERNEL_OFFSET)
    {
        *(.data.rel.ro.local*) *(.data.rel.ro .data.rel.ro.*)
        . = ALIGN(4K);
    }

    .gcc_except_table ALIGN(4K) : AT(ADDR(.gcc_except_table) - KERNEL_OFFSET)
    {
        *(.gcc_except_table)
        . = ALIGN(4K);
    }
}
