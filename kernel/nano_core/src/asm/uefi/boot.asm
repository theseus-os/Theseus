global _start

_start:
	; First argument: reference to the boot info (passed by bootloader)
	; Second argument: the top of the initial double fault stack
	mov rsi, initial_double_fault_stack_top
	call rust_entry

	; rust main returned, print `OS returned!`
	mov rdi, strings.os_return
	call eputs
	; If the system has nothing more to do, put the computer into an
	; infinite loop. To do that:
	; 1) Disable interrupts with cli (clear interrupt enable in eflags).
	;    They are already disabled by the bootloader, so this is not needed.
	;    Mind that you might later enable interrupts and return from
	;    kernel_main (which is sort of nonsensical to do).
	; 2) Wait for the next interrupt to arrive with hlt (halt instruction).
	;    Since they are disabled, this will lock up the computer.
	; 3) Jump to the hlt instruction if it ever wakes up due to a
	;    non-maskable interrupt occurring or due to system management mode.
global KEXIT
KEXIT:
	cli
.loop:
	hlt
	jmp .loop
	
section .rodata

strings:
.os_return:
	db 'OS returned',0

section .bss
low_p3_table:
	resb 4096
high_p3_table:
	resb 4096
low_p2_table:
	resb 4096
megabyte_table:
	resb 4096
kernel_table:
	resb 4096


; Note that the linker script (`linker_higher_half.lf`) inserts a 2MiB space here 
; in order to provide stack guard pages beneath the .stack section afterwards.
; We don't really *need* to specify the section itself here, but it helps for clarity's sake.
section .guard_huge_page nobits noalloc noexec nowrite


; Although x86 only requires 16-byte alignment for its stacks, 
; we use page alignment (4096B) for convenience and compatibility 
; with Theseus's stack abstractions in Rust. 
; We place the stack in its own sections for loading/parsing convenience.
; Currently, the stack is 16 pages in size, with a guard page beneath the bottom.
; ---
; Note that the `initial_bsp_stack_guard_page` is actually mapped by the boot-time page tables,
; but that's okay because we have real guard pages above. 
section .stack nobits alloc noexec write  ; same section flags as .bss
align 4096 
global initial_bsp_stack_guard_page
initial_bsp_stack_guard_page:
	resb 4096
global initial_bsp_stack_bottom
initial_bsp_stack_bottom:
	resb 4096 * INITIAL_STACK_SIZE
global initial_bsp_stack_top
initial_bsp_stack_top:
	resb 4096
global initial_double_fault_stack_top
initial_double_fault_stack_top:
