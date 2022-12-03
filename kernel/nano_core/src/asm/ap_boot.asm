%include "defines.asm"

section .init.text32ap progbits alloc exec nowrite
bits 32 ;We are still in protected mode

; extern set_up_SSE

%ifdef ENABLE_AVX
; extern set_up_AVX
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
	
	; Load the 64-bit GDT
	lgdt [GDT_AP.ptr_low - KERNEL_OFFSET]


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
%endif

; Prints `ERR: ` and the given error code to screen and hangs.
; parameter: error code (in ascii) in al
global _error
_error:
	hlt

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