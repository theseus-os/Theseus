; Declare a multiboot2-compliant header, which indicates this program is a bootable kernel image.
; This must be the first section in the kernel image, which is accomplished via our linker script. 
; It must also be aligned to a 4-byte boundary. 
section .multiboot_header ; Permissions are the same as .rodata by default
align 4
multiboot_header_start:
	dd 0xe85250d6					                   ; Multiboot2 header magic number
	dd 0							                   ; Run in protected i386 (32-bit) mode
	dd multiboot_header_end - multiboot_header_start   ; header length
	; checksum
	dd 0x100000000 - (0xe85250d6 + 0 + (multiboot_header_end - multiboot_header_start))
	; Place optional header tags here, after the checksum above. Documentation is here:
	; <https://www.gnu.org/software/grub/manual/multiboot2/multiboot.html#Header-tags>
	; Note: all tags must be aligned to 8-byte boundaries.


; Below is the framebuffer tag, used to request a graphical (non-text) framebuffer and specify its size.
; By default, we ask the bootloader to switch modes to a graphical framebuffer for us,
; though this can be disabled by defining `VGA_TEXT_MODE`.
;
; NOTE: this works properly now. Uncomment the below sections when we are ready to enable
;       early boot-time usage of the graphical framebuffer by default.
;
%ifndef VGA_TEXT_MODE
align 8
	dw 5     ; type (5 means framebuffer tag)
	dw 0     ; flags. Bit 0 = `1` means this tag is optional, Bit 0 = `0` means it's mandatory.
	dd 20    ; size of this tag (20)
	; The resolution specified below is limited by the hardware.
	; We have successfully tested resolutions up to 2560 x 1600.
	; We recommend matching the resolution set in `kernel_config/src/display.rs`.
	; See more VGA/*XGA resolutions here: <https://en.wikipedia.org/wiki/Graphics_display_resolution#Extended_Graphics_Array>
	dd 1280  ; width in pixels
	dd 1024  ; height in pixels
	dd 32    ; depth: pixel size in bits. Theseus only supports 32-bit pixels.
%endif


; This marks the end of the tag region.
align 8
	dw 0	; type (0 means terminator tag)
	dw 0	; flags
	dd 8	; size of this tag
multiboot_header_end:
