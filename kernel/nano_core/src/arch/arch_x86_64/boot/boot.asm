; Copyright 2016 Phillip Oppermann, Calvin Lee and JJ Garzella.
; See the README.md file at the top-level directory of this
; distribution.
;
; Licensed under the MIT license <LICENSE or
; http://opensource.org/licenses/MIT>, at your option.
; This file may not be copied, modified, or distributed
; except according to those terms.

; Kernel is linked to run at -2Gb
KERNEL_OFFSET equ 0xFFFFFFFF80000000

global start

; Section must have the permissions of .text
section .init.text32 progbits alloc exec nowrite
bits 32 ;We are still in protected mode
start:
	; The bootloader has loaded us into 32-bit protected mode on a x86
	; machine. Interrupts are disabled. Paging is disabled. The processor
	; state is as defined in the multiboot standard. The kernel has full
	; control of the CPU. The kernel can only make use of hardware features
	; and any code it provides as part of itself. There's no printf
	; function, unless the kernel provides its own <stdio.h> header and a
	; printf implementation. There are no security restrictions, no
	; safeguards, no debugging mechanisms, only what the kernel provides
	; itself. It has absolute and complete power over the machine.

	; To set up a stack, we set the esp register to point to the top of our
	; stack (as it grows downwards on x86 systems). This is necessarily done
	; in assembly as languages such as Rust cannot function without a stack.
	;
	; We subtract KERNEL_OFFSET from the stack address because we are not yet
	; mapped to the higher half
	mov esp, initial_bsp_stack_top - KERNEL_OFFSET

	; The multiboot2 specification requires the bootloader to load a pointer
	; to the multiboot2 information structure in the `ebx` register. Here we
	; mov it to `edi` so that rust can take it as a register. Because of this
	; we cannot clobber the edi register in any code before rust_main
	mov edi, ebx

	call check_multiboot
	call check_cpuid
	call check_long_mode

	call set_up_SSE

	call set_up_page_tables
	call enable_paging

	; Load the 64-bit GDT
	lgdt [GDT.ptr_low - KERNEL_OFFSET]

	; Load the code selector with a far jmp
	; From now on instructions are 64 bits and this file is invalid
	jmp GDT.code:long_mode_start; -> !

set_up_page_tables:
	; Set up recursive paging at the second to last entry
	mov eax, p4_table - KERNEL_OFFSET
	or eax, 11b ; present + writable
	mov [(p4_table - KERNEL_OFFSET) + (510 * 8)], eax

	; map the first P4 entry to the first p3 table
	;
	; This will be changed to the page containing
	; only the first megabyte before rust starts
	mov eax, low_p3_table - KERNEL_OFFSET
	or eax, 11b ; present + writable
	mov [p4_table - KERNEL_OFFSET], eax

	; map the last P4 entry to last P3 table
	mov eax, high_p3_table - KERNEL_OFFSET
	or eax, 11b ; present + writable
	mov [p4_table - KERNEL_OFFSET + (511 * 8)], eax

	; map first entry of the low P3 table to the kernel table
	mov eax, kernel_table - KERNEL_OFFSET
	or eax, 11b ; present + writable
	mov [low_p3_table - KERNEL_OFFSET], eax
	; now to the second to highest entry of the high P3 table
	mov [high_p3_table - KERNEL_OFFSET + (510 * 8)], eax

	; map each P2 entry to a huge 2MiB page
	mov ecx, 0x0       ; counter variable

.map_kernel_table:
	mov eax, 0x200000  ; 2MiB
	mul ecx            ; start address of ecx-th page
	or eax, 10000011b  ; present + writable + huge
	mov [(kernel_table - KERNEL_OFFSET) + (ecx * 8)], eax ; map ecx-th entry

	inc ecx            ; increase counter
	cmp ecx, 512       ; if counter == 512, the whole P2 table is mapped
	jne .map_kernel_table  ; else map the next entry

	; map the first p2 entry to the megabyte table
	mov eax, megabyte_table - KERNEL_OFFSET
	or eax, 11b
	mov [low_p2_table - KERNEL_OFFSET], eax

	; identity map the first megabyte
	mov ecx, 0x0

.map_megabyte_table:
	mov eax, 4096      ; 4Kb
	mul ecx            ; start address of ecx-th page
	or eax, 11b        ; present + writable
	mov [(megabyte_table - KERNEL_OFFSET) + (ecx * 8)], eax ; map ecx-th entry

	inc ecx            ; increase counter
	cmp ecx, 256       ; if counter = 256, the whole megabyte is mapped
	jne .map_megabyte_table ; else map the next entry

	ret

enable_paging:
	; Enable:
	;     PGE: (Page Global Extentions)
	;     PAE: (Physical Address Extension)
	;     PSE: (Physical Size Extentions)
	mov eax, cr4
	or eax, (1 << 7) | (1 << 5) | (1 << 1)
	mov cr4, eax

	; load P4 to cr3 register (cpu uses this to access the P4 table)
	mov eax, p4_table - KERNEL_OFFSET
	mov cr3, eax

	; set the no execute (bit 11), long mode (bit 8), and SYSCALL Enable (bit 0)
	; bits in the EFER MSR (model specific register)
	mov ecx, 0xC0000080
	rdmsr
	or eax, (1 <<11) | (1 << 8) | (1 << 0) ; NXE, LME, SCE
	wrmsr

	; enable paging and write protection in the cr0 register
	mov eax, cr0
	or eax, (1 << 31) | (1 << 16) ; PG | WP
	mov cr0, eax

	ret

check_multiboot:
	cmp eax, 0x36d76289
	jne .no_multiboot
	ret
.no_multiboot:
	mov al, "0"
	jmp _error

check_cpuid:
	; Check if CPUID is supported by trying to flip the ID bit (bit 21)
	; in the FLAGS register. If we can flip it, CPUID is availible
	pushfd
	pop eax

	mov ecx, eax

	;Flip ID
	xor eax, 1 << 21

	;Copy eax to FLAGS
	push eax
	popfd

	;Get and recover FLAGS
	pushfd
	pop eax
	push ecx
	popfd

	;compare the saved FLAGS
	cmp eax, ecx
	je .no_cpuid
	ret
.no_cpuid:
	mov al, "1"
	jmp _error

check_long_mode:
	; test if extended processor info in available
	mov eax, 0x80000000    ; implicit argument for cpuid
	cpuid                  ; get highest supported argument
	cmp eax, 0x80000001    ; it needs to be at least 0x80000001
	jb .no_long_mode       ; if it's less, the CPU is too old for long mode

	; use extended info to test if long mode is available
	mov eax, 0x80000001    ; argument for extended processor info
	cpuid                  ; returns various feature bits in ecx and edx
	test edx, 1 << 29      ; test if the LM-bit is set in the D-register
	jz .no_long_mode       ; If it's not set, there is no long mode
	ret
.no_long_mode:
	mov al, "2"
	jmp _error

; Check for SSE and enable it. Throw error 'a' if unsupported
set_up_SSE:
	mov eax, 0x1
	cpuid
	test edx, 1 << 25
	jz .no_SSE

	; enable SSE
	mov eax, cr0
	and ax, 0xFFFB         ; clear coprocessor emulation CRO.EM
	or ax, 0x2             ; set coprocessor monitoring CR0.MP
	mov cr0, eax

	mov eax, cr4
	or ax, 3 << 9          ; set CR4.OSFXSR and CR4.OSXMMEXCPT at the same time
	mov cr4, eax

	ret
.no_SSE:
	mov al, "a"
	jmp _error

; Prints `ERR: ` and the given error code to screen and hangs.
; parameter: error code (in ascii) in al
global _error
_error:
	mov dword [0xb8000], 0x4f524f45
	mov dword [0xb8004], 0x4f3a4f52
	mov dword [0xb8008], 0x4f204f20
	mov byte  [0xb800a], al
	hlt

; ---------------------------------------- Long Mode ----------------------------------------
bits 64
section .init.text.high
global long_mode_start
long_mode_start:
	; Load the new GDT
	lgdt [rel GDT.ptr]

	; Long jump to the higher half. Because `jmp` does not take
	; a 64 bit address (which we need because we are practically
	; jumping to address +254Tb), we must first load the address
	; to `rax` and then jump to it
	mov rax, start_high
	jmp rax

section .text
extern rust_main
extern eputs
extern puts

global start_high
start_high:
	; Set up high stack
	add rsp, KERNEL_OFFSET

	
	; for easy use of multiboot2 data structures,
	; we preserve an identity mapping that's the same as the higher-half mapping.
	; The rust code will erase the kernel's identity mapping later before jumping to userspace programs.
	;;; ; get rid of the old identity map, but
	;;; ; continue to identity map the first Mb
	;;; mov rax, low_p2_table - KERNEL_OFFSET
	;;; or rax, 11b ; present + writable
	;;; mov [rel low_p3_table], rax


	; set up the segment registers
	mov ax, GDT.data ; data offset
	mov ss, ax
	mov ds, ax
	mov es, ax
	mov fs, ax
	; mov gs, ax


	; Save the multiboot address
	push rdi
	; Load puts arguments
	mov rdi, strings.long_start
	mov si, 0x0f
	call puts
	pop rdi

	; Give rust the higher half address to the multiboot2 information structure
	add rdi, KERNEL_OFFSET
	
	call rust_main

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



; Performs the actual context switch from prev to next task.
; First argument  (rdi): mutable pointer to the previous task's stack pointer
; Second argument (rsi): the value of the next task's stack pointer
global task_switch
task_switch: 
	push rbx
	push rbp
	push r12
	push r13
	push r14
	push r15

	; save current stack pointer into prev task
	mov [rdi], rsp
	; load stack pointer from next task
	mov rsp, rsi

	pop r15
	pop r14
	pop r13
	pop r12
	pop rbp
	pop rbx
	; pops the last value off the top of the stack,
	; so the new task's stack top must point to a target function
	ret









section .rodata
; TODO TSS <http://wiki.osdev.org/Task_State_Segment>
GDT:
	dq 0 ; zero entry
.code equ $ - GDT
	dq (1<<44) | (1<<47) | (1<<41) | (1<<43) | (1<<53) ; code segment
.data equ $ - GDT
	dq (1<<44) | (1<<47) | (1<<41) ; data segment
.end equ $
.ptr_low:
	dw .end - GDT - 1
	dd GDT - KERNEL_OFFSET
.ptr:
	dw .end - GDT - 1
	dq GDT

strings:
.os_return:
	db 'OS returned',0
.long_start:
	db 'Hello long mode!',0

section .bss
; This reserves space for an empty page table to be loaded at runtime
; in set_up_tables we will make the table valid and set it up to map
; the first gigabyte of our kernel
align 4096
p4_table:
	resb 4096
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



; The multiboot standard does not define the value of the stack pointer register
; (esp) and it is up to the kernel to provide a stack. This allocates room for a
; small stack by creating a symbol at the bottom of it, then allocating 64
; bytes for it, and finally creating a symbol at the top. The stack grows
; downwards on x86. The stack is in its own section so it can be marked nobits,
; which means the kernel file is smaller because it does not contain an
; uninitialized stack. The stack on x86 must be 16-byte aligned according to the
; System V ABI standard and de-facto extensions. The compiler will assume the
; stack is properly aligned and failure to align the stack will result in
; undefined behavior.
align 16
global initial_bsp_stack_bottom
initial_bsp_stack_bottom:
	resb 4096 * 16
global initial_bsp_stack_top
initial_bsp_stack_top:
