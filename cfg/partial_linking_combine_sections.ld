/*  
 * A simple linker script to combine function and data sections into one.
 * This should be run using `ld -r`, i.e., partial relocatable linking.
 */
SECTIONS {
	.text :
	{
		*(.text .text.*)
	}

	.rodata :
	{
		*(.rodata .rodata.*)
	}

	.eh_frame :
	{
		*(.eh_frame .eh_frame.*)
	}

	.gcc_except_table :
	{
		*(.gcc_except_table .gcc_except_table.*)
	}

	.tdata :
	{
		*(.tdata .tdata.*)
	}

	.tbss :
	{
		*(.tbss .tbss.*)
	}

	.data :
	{
		*(.data .data.*)
	}

	.bss :
	{
		*(.bss .bss.*)
	}
		
}
