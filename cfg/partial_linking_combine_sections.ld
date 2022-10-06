/*  
 * A simple linker script to combine function and data sections into one.
 * This should be run using `ld -r`, i.e., partial relocatable linking.
 */
SECTIONS {

	/*
	 * A dummy section whose existence indicates to Theseus's loader/linker
	 * that the sections have been merged, and loading can thus be accelerated
	 * by using the symbol table only.
	 */
	.theseus_merged : 
	{
		PROVIDE(theseus = .);
	}
	
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

	/DISCARD/ :
	{
		*(.note .note.*)
	}
}
