%include "defines.asm"

global _start

section .init.text progbits alloc exec nowrite

_start:
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

section .text
extern rust_entry
extern eputs

section .rodata

strings:
.os_return:
	db 'OS returned',0