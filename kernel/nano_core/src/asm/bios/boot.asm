; Copyright 2016 Phillip Oppermann, Calvin Lee and JJ Garzella.
; See the README.md file at the top-level directory of this
; distribution.
;
; Licensed under the MIT license <LICENSE or
; http://opensource.org/licenses/MIT>, at your option.
; This file may not be copied, modified, or distributed
; except according to those terms.

%include "defines.asm"

global _start

; Section must have the permissions of .text
section .init.text32 progbits alloc exec nowrite
bits 32 ;We are still in protected mode

extern set_up_SSE

%ifdef ENABLE_AVX
extern set_up_AVX
%endif ; ENABLE_AVX

_start:
	; The bootloader has loaded us into 32-bit protected mode. 
	; Interrupts are disabled. Paging is disabled.

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
	; we cannot clobber the edi register in any code before nano_core_start
	mov edi, ebx

	call check_multiboot
	call check_cpuid
	call check_long_mode

	call set_up_SSE
%ifdef ENABLE_AVX
	call set_up_AVX
%endif ; ENABLE_AVX

	call set_up_page_tables
	call unmap_guard_page
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
	mul ecx            ; eax now holds the start address of the ecx-th page
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


unmap_guard_page:
	; put the address of the stack guard huge pages into ecx
	mov ecx, (initial_bsp_stack_guard_page - 0x200000 - KERNEL_OFFSET)
	shr ecx, 18      ; calculate p2 index
	and ecx, 0x1FF  ; get p2 index by itself
	; ecx now holds the index into the p2 page table of the entry we want to unmap
	mov eax, 0x0  ; set huge page flag, clear all others
	mov [(kernel_table - KERNEL_OFFSET) + ecx], eax ; unmap (clear) ecx-th entry
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
extern rust_entry
extern eputs
extern puts
extern KEXIT

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
	mov gs, ax

	; set the IA32_TSC_AUX MSR to a sentry value, in order to prevent
	; invalid usage of its value before Theseus sets it to the CPU's APIC ID.
	mov eax, 0xFFFFFFFF
	mov edx, 0x0
	mov ecx, 0xc0000103   ; IA32_TSC_AUX MSR
	wrmsr
	
	; clear out the FS/GS base MSRs
	xor eax, eax          ; set to 0
	xor edx, edx          ; set to 0
	mov ecx, 0xc0000100   ; FS BASE MSR
	wrmsr
	mov ecx, 0xc0000101   ; GS BASE MSR
	wrmsr
	mov ecx, 0xc0000102   ; GS KERNEL BASE MSR
	wrmsr


	; Save the multiboot address
	push rdi
	; Load puts arguments
	mov rdi, strings.long_start
	mov si, 0x0f
	call puts
	pop rdi

	; First argument: the higher half address to the multiboot2 information structure
	add rdi, KERNEL_OFFSET
	; Second argument: the top of the initial double fault stack
	mov rsi, initial_double_fault_stack_top
	call rust_entry
	jmp KEXIT


section .rodata
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
.long_start:
	db 'Hello long mode!',0


; The following `resb` commands reserve space for the first page table,
; which we must set up before enabling paging and jumping to long mode.
; We split it into two parts:
; (1) the initial p4 page table (the root P4 frame), and
; (2) all the other initial page table frames. 
; This is because Theseus needs to obtain exclusive ownership of the root p4 table
; separately from the rest of the .data/.bss section contents.
section .page_table nobits alloc noexec write  ; same section flags as .bss
align 4096 
p4_table:
	resb 4096

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
initial_double_fault_stack_top:
