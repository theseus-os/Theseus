%include "defines.asm"

section .init.text32ap progbits alloc exec nowrite
bits 32 ;We are still in protected mode

extern set_up_SSE

%ifdef ENABLE_AVX
extern set_up_AVX
%endif

global ap_start_protected_mode
ap_start_protected_mode:
	; xchg bx, bx ; bochs magic breakpoint
	 
	mov esp, 0xFC00; set a new stack pointer, 512 bytes below our AP_STARTUP code region

	call set_up_SSE ; in boot.asm

%ifdef ENABLE_AVX
	call set_up_AVX ; in boot.asm
%endif
    
	call set_up_paging_ap
	

    ; each character is reversed in the dword cuz of little endianness
	; prints PGTBL
	mov dword [0xb8018], 0x4f2E4f2E ; ".."
    mov dword [0xb801c], 0x4f504f2E ; ".P"
	mov dword [0xb8020], 0x4f544f47 ; "GT"
	mov dword [0xb8024], 0x4f4C4f42 ; "BL"

	; Load the 64-bit GDT
	lgdt [GDT_AP.ptr_low - KERNEL_OFFSET]


	; prints GDT
	mov dword [0xb8028], 0x4f2E4f2E ; ".."
    mov dword [0xb802c], 0x4f474f2E ; ".G"
	mov dword [0xb8030], 0x4f544f44 ; "DT"
	mov eax, 0x4f004f00
	or eax, GDT_AP.code + 0x30 ; convert GDT_AP.code value to ASCII char
	mov dword [0xb8034], eax ; prints GDT_AP.code value


	; Load the code selector via a far jmp
	; From now on instructions are 64 bits
	jmp dword GDT_AP.code:long_mode_start_ap; -> !

	; an alternative to jmp, we construct a jmp instr on the stack. No difference though.
	; push 8
	; push long_mode_start_ap
	; retf



set_up_paging_ap:
	; first, quickly disable paging
	mov eax, cr0
	and eax, 0x7FFFFFFF ; clear bit 31
	mov cr0, eax

	; Enable:
	;     PGE: (Page Global Extentions)
	;     PAE: (Physical Address Extension)
	;     PSE: (Physical Size Extentions)
	mov eax, cr4
	or eax, (1 << 7) | (1 << 5) ; | (1 << 1)
	mov cr4, eax

	; load P4 to cr3 register (cpu uses this to access the P4 table)
    ; to set up paging for the newly-booted AP, 
    ; use the same page table that the BSP Rust code set up for us in the trampoline
	mov eax, [AP_PAGE_TABLE]
	mov cr3, eax

	; set the no execute (bit 11), long mode (bit 8), and SYSCALL Enable (bit 0)
	; bits in the EFER MSR (which is MSR 0xC0000080)
	mov ecx, 0xC0000080
	rdmsr
	or eax, (1 <<11) | (1 << 8) | (1 << 0) ; NXE, LME, SCE
	wrmsr

	; enable paging and write protection in the cr0 register
	mov eax, cr0
	or eax, (1 << 31) | (1 << 16) | (1 << 0); PG | WP | PM
	mov cr0, eax

    ret



; ---------------------------------------- Long Mode ----------------------------------------
bits 64
section .init.text.highap
global long_mode_start_ap
long_mode_start_ap:
	; in long mode, it's okay to set data segment registers to 0
	; ; mov rax, GDT_AP.data
	mov ax, 0
	mov ss, ax
	mov ds, ax
	mov es, ax
	mov fs, ax
	mov gs, ax


	; Load the new GDT
	lgdt [rel GDT_AP.ptr]


	; mov rsp, 0xFC00
	

	; each character is reversed in the dword cuz of little endianness
	mov dword [0xFFFFFFFF800b8038], 0x4f2E4f2E ; ".."
    mov dword [0xFFFFFFFF800b803c], 0x4f4f4f4c ; "LO"
	mov dword [0xFFFFFFFF800b8040], 0x4f474f4e ; "NG"

	; Long jump to the higher half. Because `jmp` does not take
	; a 64 bit address (which we need because we are practically
	; jumping to address +254Tb), we must first load the address
	; to `rax` and then jump to it
	mov rax, start_high_ap
	jmp rax


section .text.ap

global start_high_ap
start_high_ap:
	cli
	; Set up high stack
	; add rsp, KERNEL_OFFSET

	; set up the segment registers
	mov ax, 0  ; a null (0) data segment selector is fine for 64-bit instructions
	mov ss, ax
	mov ds, ax
	mov es, ax
	mov fs, ax
	mov gs, ax
	
	; each character is reversed in the dword cuz of little endianness
	mov dword [0xb8048 + KERNEL_OFFSET], 0x4f2E4f2E ; ".."
    mov dword [0xb804c + KERNEL_OFFSET], 0x4f494f48 ; "HI"
	mov dword [0xb8050 + KERNEL_OFFSET], 0x4f484f47 ; "GH"
	mov dword [0xb8054 + KERNEL_OFFSET], 0x4f524f45 ; "ER"

	; move to the new stack that was alloc'd for this AP
	mov rcx, [AP_STACK_END]
	lea rsp, [rcx - 256]

    ; Rust's calling conventions is as follows:  
	; RDI,  RSI,  RDX,  RCX,  R8,  R9,  (R10??), others on stack
	; This order below MUST MATCH the parameter order in kstart_ap()
	mov rdi, [AP_PROCESSOR_ID]
	mov rsi, [AP_APIC_ID]
	mov rdx, [AP_STACK_START]
	mov rcx, [AP_STACK_END]
	mov r8,  [AP_NMI_LINT]
	mov r9,  [AP_NMI_FLAGS]
	mov rax, qword [AP_CODE]


	; we signal the BSP that we're booting into Rust code, 
	; and that we're done using the trampoline space
	mov qword [AP_READY], 1
	jmp rax


	; If the Rust code returned, which is an error, 
	; then put the core into an infinite loop.
	cli
.loop:
	hlt
	jmp .loop


; One would expect the GDT to be in rodata, since you shouldn't need to write to it.
; However, during the ap boot phase on real hardware, there is a write page fault
; if you put it in rodata (i.e., map it as read-only).
section .data.ap
GDT_AP:
	dq 0 ; zero entry
.code equ $ - GDT_AP
	dq (1<<44) | (1<<47) | (1<<41) | (1<<43) | (1<<53) ; code segment
	; dq (1<<44) | (1<<47) | (1<<43) | (1<<53) | 0xFFFF; code segment, limit 0xFFFF
.data equ $ - GDT_AP
	dq (1<<44) | (1<<47) | (1<<41) ; | (1 << 53) ; data segment
.end equ $
; ALIGN 4
; 	dw 0 ; padding to make sure GDT pointer is 4-byte aligned
.ptr_low:
	dw .end - GDT_AP - 1
	dd GDT_AP - KERNEL_OFFSET
	; dq GDT_AP - KERNEL_OFFSET
.ptr:
	dw .end - GDT_AP - 1
	dq GDT_AP