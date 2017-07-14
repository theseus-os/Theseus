; Copyright 2016 Phillip Oppermann, Calvin Lee and JJ Garzella.
; See the README.md file at the top-level directory of this
; distribution.
;
; Licensed under the MIT license <LICENSE or
; http://opensource.org/licenses/MIT>, at your option.
; This file may not be copied, modified, or distributed
; except according to those terms.

section .text
bits 64
extern KEXIT
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
