%include "defines.asm"

%ifdef BIOS
ABSOLUTE 0x5000
VBECardInfo:
	.signature             resb 4
	.version               resw 1
	.oemstringptr          resd 1
	.capabilities          resd 1
	.videomodeptr          resd 1
	.totalmemory64KiB      resw 1
	.oemsoftwarerev        resw 1
	.oemvendornameptr      resd 1
	.oemproductnameptr     resd 1
	.oemproductrevptr      resd 1
	.reserved              resb 222
	.oemdata               resb 256

ABSOLUTE 0x5200
VBEModeInfo:
    ; All VBE versions have the following
	.attributes            resw 1
	.winAattr              resb 1
	.winBattr              resb 1
	.wingranularity        resw 1
	.winsize               resw 1
	.winAsegment           resw 1
	.winBsegment           resw 1
	.winfuncptr            resd 1
	.bytesperscanline      resw 1
    ; VBE 1.2 and above has the following
	.width                 resw 1
	.height                resw 1
	.xcharsize             resb 1
	.ycharsize             resb 1
	.numberofplanes        resb 1
	.bitsperpixel          resb 1
	.numberofbanks         resb 1
	.memorymodel           resb 1
	.banksizeKiB           resb 1
	.numberofimagepages    resb 1
	.reserved0             resb 1
	.redmasksize           resb 1
	.redfieldposition      resb 1
	.greenmasksize         resb 1
	.greenfieldposition    resb 1
	.bluemasksize          resb 1
	.bluefieldposition     resb 1
	.rsvdmasksize          resb 1
	.rsvdfieldposition     resb 1
	.directcolormodeinfo   resb 1
    ; VBE 2.0 and above has the following
	.physbaseptr           resd 1
	.reserved1             resd 1
	.reserved2             resw 1
    ; VBE 3.0 and above has the following (currently unused)
    .reserved_vbe3         resb 13
	.reserved3             resb 189

ABSOLUTE 0x5400
current:
    .mode                  resw 1

; Keeps track of the "best" (highest-resolution) graphics mode so far
best_mode:
    .mode                  resw 1
    .width                 resw 1
    .height                resw 1
    .physaddr              resd 1
	.attributes            resw 1
	.totalmemory64KiB      resw 1
	.bytesperscanline      resw 1
	.bitsperpixel          resb 1
    .redmasksize           resb 1
	.redfieldposition      resb 1
	.greenmasksize         resb 1
	.greenfieldposition    resb 1
	.bluemasksize          resb 1
	.bluefieldposition     resb 1
%endif ; BIOS

section .init.realmodetext16 progbits alloc exec nowrite
bits 16 ; we're in real mode, that's how APs boot up

; This is the entry point for APs when they first boot.
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

%ifdef BIOS
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

; We only need to execute the graphic mode setting code once, system-wide. 
; We use the byte at [0000:0900] to indicate if another core has already run this code.
    mov ax, 0
    mov es, ax
    mov di, 0x900
    mov ax, [es:di]
    cmp ax, 5
    ; skip graphics mode setting code if it's already been done.
    je create_gdt


; This is the start of the graphics mode code. 
; First, zero-out the key fields of our "best" mode info and the Rust-visible GraphicInfo.
    mov word  [best_mode.mode],     0
    mov word  [best_mode.width],    0
    mov word  [best_mode.height],   0
    mov dword [best_mode.physaddr], 0
    
    ; WARNING: the below code must be kept in sync with the `GraphicInfo` struct 
    ;          in the `multicore_bringup` crate. 
    push di
    mov ax, 0
    mov es, ax
    mov di, 0xF100  ; see `GRAPHIC_INFO_OFFSET_FROM_TRAMPOLINE`
    ; set key fields of `GraphicInfo` to zero
    mov word  [es:di+0],  0   ; width
    mov word  [es:di+2],  0   ; height
    mov dword [es:di+4],  0   ; physical address 
    pop di

; Next, we get the VBE card info such that we can iterate over the list of available modes. 
; We then pick the highest-resolution mode, only considering modes with 32-bit pixels. 
; If this runs successfully, we write the graphics mode details starting at physical address 0xF100
; such that Rust code can read them later. 
get_vbe_card_info:
    mov ax, 0x4F00         ; 0x4F00 is the argument to the BIOS 0x10 interrupt used to request VGA/VESA information
    mov di, VBECardInfo    ; Set `di` to the address where the VBE card info will be written
    int 0x10
    cmp al, 0x4F           ; The result is placed into `al`. A result of `0x4f` means the query was successful.
    jne graphic_mode_done  ; A failure here means we can't set graphic modes, so just skip to the end. 
    
    ; initialize the mode pointer so we can iterate over the available graphics modes
    mov si, [VBECardInfo.videomodeptr]
    mov ax, [VBECardInfo.videomodeptr+2]
    mov fs, ax          ; [fs:si] will point to the first mode in the list of modes
    sub si, 2           ; initilize the pointer [fs:si] to the index before the first mode

; Traverse the next mode in the list of available modes
.next_mode:
    add si, 2           ; [fs:si] points to the next mode
    mov cx, [fs:si]     ; `cx` now holds the current mode index
    cmp cx, 0xFFFF      ; A mode index of 0xFFFF indicates we are done iterating over the modes.
    je mode_iter_done

    ; Here, we attempt to get the mode info for the mode we just iterated to
    push esi
    mov [current.mode], cx  ; Store the current mode in `current.mode`
    mov ax, 0x4F01          ; 0x4F01 is the argument fo the BIOS 0x10 interrupt used to get the currrent mode information
    mov di, VBEModeInfo     ; Set `di` to the address where the mode information will be written
    int 0x10
    pop esi
    cmp al, 0x4F            ; The result is placed into `al`. A result of `0x4f` means the query was successful.
    jne .next_mode          ; We failed to get info about this mode. Go back and try the next mode.

    ; We only support modes with 32-bit pixel sizes
    cmp byte [VBEModeInfo.bitsperpixel], 32
    jne .next_mode
    ; Check whether the current mode is higher resolution than our maximum resolution.
    ; If it is, then continue iterating through the modes.
    mov word ax, [VBEModeInfo.width]
    cmp word ax, [es:AP_MAX_FB_WIDTH]
    ja .next_mode
    mov word ax, [VBEModeInfo.height]
    cmp word ax, [es:AP_MAX_FB_HEIGHT]
    ja .next_mode
    ; Check whether the current mode is higher resolution than the "best" mode thus far.
    ; If not, continue iterating through the modes. 
    mov word ax, [best_mode.width]
    cmp word [VBEModeInfo.width], ax
    jb .next_mode ; if current width is greater than or equal to best width, fall through.
    mov word ax, [best_mode.height]
    cmp word [VBEModeInfo.height], ax
    jb .next_mode

    ; Here, the current mode was higher resolution, so we update the "best" mode to that.
    mov word ax, [current.mode]
    mov word [best_mode.mode], ax
    mov word ax, [VBEModeInfo.width]
    mov word [best_mode.width], ax
    mov word ax, [VBEModeInfo.height]
    mov word [best_mode.height], ax
    mov word ax, [VBEModeInfo.physbaseptr]
    mov word [best_mode.physaddr], ax
    mov word ax, [VBEModeInfo.physbaseptr+2]
    mov word [best_mode.physaddr+2], ax
    mov word ax, [VBEModeInfo.attributes]
    mov word [best_mode.attributes], ax
    mov word ax, [VBECardInfo.totalmemory64KiB]
    mov word [best_mode.totalmemory64KiB], ax
    mov word ax, [VBEModeInfo.bytesperscanline]
    mov word [best_mode.bytesperscanline], ax
    mov byte al, [VBEModeInfo.bitsperpixel]
    mov byte [best_mode.bitsperpixel], al
    mov byte al, [VBEModeInfo.redmasksize]
    mov byte [best_mode.redmasksize], al
    mov byte al, [VBEModeInfo.redfieldposition]
    mov byte [best_mode.redfieldposition], al
    mov byte al, [VBEModeInfo.greenmasksize]
    mov byte [best_mode.greenmasksize], al
    mov byte al, [VBEModeInfo.greenfieldposition]
    mov byte [best_mode.greenfieldposition], al
    mov byte al, [VBEModeInfo.bluemasksize]
    mov byte [best_mode.bluemasksize], al
    mov byte al, [VBEModeInfo.bluefieldposition]
    mov byte [best_mode.bluefieldposition], al
    jmp .next_mode    ; we may find better modes later, so keep iterating!

; Once we have iterated over all available graphic modes, we jump here to
; store the details of the "best" mode that we found into [0000:F100]
; such that our Rust code can access the details of the current graphic mode.
mode_iter_done:
    ; WARNING: the below code must be kept in sync with the `GraphicInfo` struct 
    ;          in the `multicore_bringup` crate. 
    push di
    mov ax, 0
    mov es, ax
    mov di, 0xF100
    ; copy the best mode's width to [0:F100]
    mov word ax, [best_mode.width]
    mov word [es:di+0], ax
    ; copy the best mode's height to [0:F102]
    mov word ax, [best_mode.height]
    mov word [es:di+2], ax
    ; move the best mode's 32-bit physical address to [0:F104]
    mov word ax, [best_mode.physaddr]
    mov word [es:di+4], ax
    mov word ax, [best_mode.physaddr+2]
    mov word [es:di+6], ax
    ; move the best mode's mode value to [0:F108]
    mov word ax, [best_mode.mode]
    mov word [es:di+8], ax
    ; move the best mode's attributes value to [0:F10A]
    mov word ax, [best_mode.attributes]
    mov word [es:di+10], ax
    ; move the best mode's totalmemory64KiB value to [0:F10C]
    mov word ax, [best_mode.totalmemory64KiB]
    mov word [es:di+12], ax
    ; move the best mode's bytesperscanline value to [0:F10E]
    mov word ax, [best_mode.bytesperscanline]
    mov word [es:di+14], ax
    ; move the best mode's bitsperpixel value to [0:F110]
    mov byte al, [best_mode.bitsperpixel]
    mov byte [es:di+16], al
    ; move the best mode's redmasksize value to [0:F111]
    mov byte al, [best_mode.redmasksize]
    mov byte [es:di+17], al
    ; move the best mode's redfieldposition value to [0:F112]
    mov byte al, [best_mode.redfieldposition]
    mov byte [es:di+18], al
    ; move the best mode's greenmasksize value to [0:F113]
    mov byte al, [best_mode.greenmasksize]
    mov byte [es:di+19], al
    ; move the best mode's greenfieldposition value to [0:F114]
    mov byte al, [best_mode.greenfieldposition]
    mov byte [es:di+20], al
    ; move the best mode's bluemasksize value to [0:F115]
    mov byte al, [best_mode.bluemasksize]
    mov byte [es:di+21], al
    ; move the best mode's bluefieldposition value to [0:F116]
    mov byte al, [best_mode.bluefieldposition]
    mov byte [es:di+22], al
    ; Note: the size of `GraphicInfo` is currently 23 bytes.
    pop di

; Finally, once we have saved the info of the best graphical mode,
; we must actually set the VGA card to use that mode. 
; For this, we use BIOS int 0x10 with arguments `0x4F02` in `ax`
; and the chosen mode index in `bx`. 
    mov ax, 0x4F02
    mov bx, [best_mode.mode]
    int 0x10

graphic_mode_done:
    ; Set the byte at [0000:0900] to indicate that we've run the graphic mode setting code.
    mov ax, 0
    mov es, ax
    mov di, 0x900
    mov byte [es:di], 5
    ; move on (fall through) to the next step, setting up our GDT
%endif ; BIOS


; Here, we create a GDT manually by writing its contents directly, starting at address 0x800.
; Since this code will be forcibly loaded by the GRUB multiboot2 bootloader at an address above 1MB,
; and because we're in 16-bit real mode, we cannot create a GDT regularly using assembler directives.
create_gdt:
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
    ; `0x8` is the newly-created kernel code segment from the above GDT
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

%ifdef BIOS
    ; each character is reversed in the dword cuz of little endianness
    ; prints "AP_PROTECTED"
    mov dword [0xb8000], 0x4f504f41 ; "AP"
    mov dword [0xb8004], 0x4f504f5F ; "_P"
    mov dword [0xb8008], 0x4f4f4f52 ; "RO"
    mov dword [0xb800c], 0x4f454f54 ; "TE"
    mov dword [0xb8010], 0x4f544f43 ; "CT"
    mov dword [0xb8014], 0x4f444f45 ; "ED"
%endif ; BIOS

    jmp 0x08:ap_start_protected_mode
    

halt:
    jmp halt



global ap_start_realmode_end
ap_start_realmode_end:
    nop