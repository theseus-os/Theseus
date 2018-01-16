section .init.text16 progbits alloc exec nowrite
; align 4096
bits 16 ; we're in real mode, that's how APs boot up
top:

extern _error

global ap_startup_start
ap_startup_start:

    ; we use real mode segment addressing here
    ; in which PhysicalAddr = Segment * 16 + Offset
    ; Address is <SegmentHex:OffsetHex>, so B000:8000 => 0xB8000
    ; mov ax, 0xB000
    ; mov ds, ax     ; you can't move an immediate value directly into a segment register like "ds"
    ; mov si, 0x8000
    ; mov eax, 0x4f4E4f4f
    ; stdsd ; store double word from eax into segment address ds:si 
    
    ;mov [0xB000:0x8000], 0x4f4E ; "AP"
    ;mov [0xb8002], 0x4f4F ; "P"

    ; need to use BIOS interrupts to write to vga buffer, not mem-mapped 0xb8000
    mov ah, 0x0E
    mov al, "A"
    int 0x10

halt:
    jmp halt




; this is unnecessary since we do a byte-wise copy of this code
; fill the rest of the page with 0s
; times 4096 - ($-top) db 0 

global ap_startup_end
ap_startup_end:
    nop