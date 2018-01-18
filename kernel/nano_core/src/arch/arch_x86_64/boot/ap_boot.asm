global ap_start_protected_mode

section .init.text32 progbits alloc exec nowrite
bits 32 ;We are still in protected mode
ap_start_protected_mode:
    jmp $


    call set_up_paging_ap

	; Load the 64-bit GDT
	; lgdt [GDT.ptr_low - KERNEL_OFFSET]

	; Load the code selector with a far jmp
	; From now on instructions are 64 bits and this file is invalid
	; jmp GDT.code:long_mode_start; -> !




set_up_paging_ap:
    ; to set up paging for the newly-booted AP, 
    ; use the same page table that the BSP Rust code set up for us in the trampoline


    ret
