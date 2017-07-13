; Copyright 2016 Phillip Oppermann, Calvin Lee and JJ Garzella.
; See the README.md file at the top-level directory of this
; distribution.
;
; Licensed under the MIT license <LICENSE or
; http://opensource.org/licenses/MIT>, at your option.
; This file may not be copied, modified, or distributed
; except according to those terms.

; Declare a multiboot header that marks the program as a kernel. These are magic
; values that are documented in the multiboot standard. The bootloader will
; search for this signature in the first 8 KiB of the kernel file, aligned at a
; 32-bit boundary. The signature is in its own section so the header can be
; forced to be within the first 8 KiB of the kernel file.
section .multiboot_header ; Permissions are the same as .rodata by default
align 4
header_start:
	dd 0xe85250d6					;Multiboot2 magic number
	dd 0							;Run in protected i386 mode
	dd header_end - header_start	;header length
	;check sum
	dd 0x100000000 - (0xe85250d6 + 0 + (header_end - header_start))

	;optional tags

	;end tags
	dw 0	;type
	dw 0	;flags
	dd 8	;size
header_end:
