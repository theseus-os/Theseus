ABSOLUTE 0x5000
VBECardInfo:
	.signature resb 4
	.version resw 1
	.oemstring resd 1
	.capabilities resd 1
	.videomodeptr resd 1
	.totalmemory resw 1
	.oemsoftwarerev resw 1
	.oemvendornameptr resd 1
	.oemproductnameptr resd 1
	.oemproductrevptr resd 1
	.reserved resb 222
	.oemdata resb 256

ABSOLUTE 0x5200
VBEModeInfo:
	.attributes resw 1
	.winA resb 1
	.winB resb 1
	.granularity resw 1
	.winsize resw 1
	.segmentA resw 1
	.segmentB resw 1
	.winfuncptr resd 1
	.bytesperscanline resw 1
	.xresolution resw 1
	.yresolution resw 1
	.xcharsize resb 1
	.ycharsize resb 1
	.numberofplanes resb 1
	.bitsperpixel resb 1
	.numberofbanks resb 1
	.memorymodel resb 1
	.banksize resb 1
	.numberofimagepages resb 1
	.unused resb 1
	.redmasksize resb 1
	.redfieldposition resb 1
	.greenmasksize resb 1
	.greenfieldposition resb 1
	.bluemasksize resb 1
	.bluefieldposition resb 1
	.rsvdmasksize resb 1
	.rsvdfieldposition resb 1
	.directcolormodeinfo resb 1
	.physbaseptr resd 1
	.offscreenmemoryoffset resd 1
	.offscreenmemsize resw 1
	.reserved resb 206

ABSOLUTE 0x5400
current:
    .mode resd 1

section .init.realmodetext16 progbits alloc exec nowrite
bits 16 ; we're in real mode, that's how APs boot up

global ap_start_realmode

ap_start_realmode:
    cli

    xor ax, ax
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax
    mov ss, ax

    mov sp, 0xFC00  ; top of stack, provided by bootloader (GRUB)

    ; we use real mode segment addressing here
    ; in which PhysicalAddr = Segment * 16 + Offset
    ; Address is <SegmentHex:OffsetHex>, so B000:8000 => 0xB8000

    ; need to use BIOS interrupts to write to vga buffer, not mem-mapped 0xb8000
    mov ah, 0x0E
    mov al, "A"
    int 0x10
    mov al, "P"
    int 0x10
    mov al, " "
    int 0x10
    mov al, "B"
    int 0x10
    mov al, "O"
    int 0x10
    mov al, "O"
    int 0x10
    mov al, "T"
    int 0x10

    mov ax, 0
    mov es, ax
    mov di, 0x900
    mov ax, [es:di]
    cmp ax, 5
    je gdt

getcardinfo:
    mov ax, 0x4F00
    mov di, VBECardInfo
    int 0x10
    cmp ax, 0x4F
    jne gdt
    
findmode:
    mov si, [VBECardInfo.videomodeptr]
    mov ax, [VBECardInfo.videomodeptr+2]
    mov fs, ax
    sub si, 2

.searchmodes:
    add si, 2
    mov cx, [fs:si]
    cmp cx, 0xFFFF
    je store_mode_info

.getmodeinfo:
    push esi
    mov [current.mode], cx
    mov ax, 0x4F01
    mov di, VBEModeInfo
    int 0x10
    pop esi
    cmp ax, 0x4F
    jne store_mode_info

.foundmode:
    ;check minimum values, really not minimums from an OS perspective but ugly for users
    cmp byte [VBEModeInfo.bitsperpixel], 32
    jb .searchmodes
    cmp byte [VBEModeInfo.xresolution], 600
    jb .searchmodes

store_mode_info:    
    push di
    mov ax, 0
    mov es, ax
    mov di, 0xF100

    ;x resolution
    mov word ax, [VBEModeInfo.xresolution]
    mov word [es:di], ax
    mov word [es:di+2], 0
    mov word [es:di+4], 0
    mov word [es:di+6], 0

    ;y resolution
    mov word ax, [VBEModeInfo.yresolution]
    mov word [es:di+8], ax
    mov word [es:di+10], 0
    mov word [es:di+12], 0
    mov word [es:di+14], 0

    ;liner frame buffer address
    mov word ax, [VBEModeInfo.physbaseptr]
    mov word [es:di+16], ax
    mov word ax, [VBEModeInfo.physbaseptr+2]
    mov word [es:di+18], ax
    mov word [es:di+20], 0000
    mov word [es:di+22], 0000
    pop di

set_graphic_mode:
    mov ax, 0x4f02;bx 4___ is linear frame buffer 
    mov bx, [current.mode] ; 0x4f41:640*400*32bit
    int 0x10;

    push ds
    push es
    mov ax,0x1103
    mov  bh,6
    int 0x10
    push es
    pop ds
    pop es
    mov si,bp
    mov cx,256*16/4
    rep movsd
    pop ds
    
    mov ax, 0
    mov es, ax
    mov di, 0x900
    mov byte [es:di], 5

gdt:
    ; here we're creating a GDT manually at address 0x800 by writing to addresses starting at 0x800
    ; since this code will be forcibly loaded by GRUB multiboot above 1MB, and we're in 16-bit real mode,
    ; we cannot create a gdt regularly. We have to 
    ; Point es:di to the right memory section:
    mov   ax, 0
    mov   es, ax     ; segment of 0 since we're just accessing 0x800
    mov   di, 0x800  ; the starting 16-bit real mode address of the GDT
    
    ; NULL Descriptor:
    mov   cx, 4                         ; Write the NULL descriptor,
    rep   stosw                         ; which is 4 zero-words.
    
    ; Kernel Code segment descriptor:
    mov   word [es:di],   0xffff    ; limit = 0xffff (since granularity bit is set, this is 4 GB)
    mov   word [es:di+2], 0x0000    ; base = 0x0000
    mov   byte [es:di+4], 0x0       ; base
    mov   byte [es:di+5], 0x9a      ; access = 0x9a (see above)
    mov   byte [es:di+6], 0xcf      ; flags + limit = 0xcf (see above)
    mov   byte [es:di+7], 0x00      ; base
    add   di, 8
    
    ; Kernel Data segment descriptor:
    mov   word [es:di],   0xffff    ; limit = 0xffff (since granularity bit is set, this is 4 GB)
    mov   word [es:di+2], 0x0000    ; base = 0x0000
    mov   byte [es:di+4], 0x0       ; base
    mov   byte [es:di+5], 0x92      ; access = 0x92 (see above)
    mov   byte [es:di+6], 0xcf      ; flags + limit = 0xcf (see above)
    mov   byte [es:di+7], 0x00      ; base
    add   di, 8

gdt_ptr:
    mov  word  [es:di],    23       ; Size of GDT in bytes minus 1
    mov  dword [es:di+2],  0x800    ; Linear address of GDT
 
load_gdt:
    lgdt [es:di]        ; es:di is the addr of gdt pointer

;     ; i don't think we need to enable A20-line, since we already did that in GRUB/BSP boot
;     ; in al, 0x92
;     ; or al, 2
;     ; out 0x92, al

    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov ss, ax


    ; finally enable protected mode
    mov eax, cr0
    or eax, 1      ; just set bit 0
    mov cr0, eax 

    ; simply jumping a little bit further clears out any pre-fetched 16-bit instructions in the pipeline
    jmp clear_prefetch
    nop
    nop
clear_prefetch: 
    ; jump to protected mode. "dword" here tells nasm to generate a 32-bit instruction,
    ; even though we're still in 16-bit mode. GCC's "as" assembler can't do that! haha
    ; 0x8 is for the newly-created kernel code segment from the above GDT
    jmp dword 0x8:prot_mode 


extern ap_start_protected_mode

section .init.text32ap progbits alloc exec
bits 32
prot_mode:

    ; set up new segment selectors. Code selector is already set correctly)
    ; GDT: kernel code is 0x08, kernel data is 0x10
    mov ax, 0x10   
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax
    mov ss, ax

    ; each character is reversed in the dword cuz of little endianness
    ; prints "AP_PROTECTED"
    mov dword [0xb8000], 0x4f504f41 ; "AP"
    mov dword [0xb8004], 0x4f504f5F ; "_P"
    mov dword [0xb8008], 0x4f4f4f52 ; "RO"
    mov dword [0xb800c], 0x4f454f54 ; "TE"
    mov dword [0xb8010], 0x4f544f43 ; "CT"
    mov dword [0xb8014], 0x4f444f45 ; "ED"
    
 
    jmp 0x08:ap_start_protected_mode
    

halt:
    jmp halt






; real_mode_stack_bottom:
;     resb 512
; real_mode_stack_top:




global ap_start_realmode_end
ap_start_realmode_end:
    nop