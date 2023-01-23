; Copyright 2016 Phillip Oppermann, Calvin Lee and JJ Garzella.
; See the README.md file at the top-level directory of this
; distribution.
;
; Licensed under the MIT license <LICENSE or
; http://opensource.org/licenses/MIT>, at your option.
; This file may not be copied, modified, or distributed
; except according to those terms.


section .init.text32ap progbits alloc exec nowrite
bits 32 ;We are still in protected mode
extern _error

; Check for SSE and enable it. Prints error 'a' if unsupported
global set_up_SSE
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


; Check for AVX and enable it. Prints error 'b' if unsupported
%ifdef ENABLE_AVX
global set_up_AVX
set_up_AVX:
	; check architectural support
	mov eax, 0x1
	cpuid
	test ecx, 1 << 26	; is XSAVE supported?
	jz .no_AVX
	test ecx, 1 << 28	; is AVX supported?
	jz .no_AVX

	; enable OSXSAVE
	mov eax, cr4
	or eax, 1 << 18		; enable OSXSAVE
	mov cr4, eax

	; enable AVX
	mov ecx, 0
	xgetbv
	or eax, 110b		; enable SSE and AVX
	mov ecx, 0
	xsetbv

	ret
.no_AVX:
	mov al, "b"
	jmp _error
%endif ; ENABLE_AVX

section .text
bits 64

; We follow the System V calling conventions, which rust uses, in order to
; get and return arguments. In general, all calling arguments are passed in
; rdi, rsi, rdx, rcx( or r10?), r8 and r9 or varients thereof (the first 32
; bit argument will be passed in edi, the first 16 in di, and the first 8 in
; di as well) and the return value is passed in rax.
; All registers except RBP, RBX, and r12-r15 are caller preserved :)


; Error puts function for long mode, if we
; ever need to extend the file to need it
; result: printf("ERROR: %s",rdi);
global eputs
eputs:
	;0x04, red on black.
	mov rax, 0x044F045204520445
	mov [0xb8000], rax
	mov rax, 0x00000420043a0452
	mov [0xb8008], rax
	;prepare to "call" puts
	mov si, 0x04 ; Red on black
	; Push cannot take a 64bit argument
	mov rax, KEXIT
	push rax ; Makes puts ret to KEXIT

; Regular puts, is called with a pointer
; to a string and a color byte.
global puts
puts:
	mov rcx, 0xb800e
	mov dx, si
.loop:
	mov al, [rdi]

	test al, al
	jz .end

	;char
	mov byte [rcx], al
	inc rcx
	;color
	mov byte [rcx], dl
	inc rcx
	inc rdi
	jmp .loop
.end:
	ret

global KEXIT
KEXIT:
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

	cli
.loop:
	hlt
	jmp .loop
	
section .rodata
strings:
.os_return:
	db 'OS returned',0
