%ifndef __DEFINES_ASM
%define __DEFINES_ASM

; This must match the `ApTrampolineData` struct definitions used in `bring_up_ap()`.
; See that struct definition for an explanation of how these are used.
; The following are physical addresses.
TRAMPOLINE          equ 0xF000
AP_READY            equ TRAMPOLINE + 0
AP_PROCESSOR_ID     equ TRAMPOLINE + 8
AP_APIC_ID          equ TRAMPOLINE + 16
AP_PAGE_TABLE       equ TRAMPOLINE + 24
AP_STACK_START      equ TRAMPOLINE + 32
AP_STACK_END        equ TRAMPOLINE + 40
AP_CODE             equ TRAMPOLINE + 48
AP_NMI_LINT         equ TRAMPOLINE + 56
AP_NMI_FLAGS        equ TRAMPOLINE + 64
AP_MAX_FB_WIDTH     equ TRAMPOLINE + 72
AP_MAX_FB_HEIGHT    equ TRAMPOLINE + 80
AP_GDT              equ TRAMPOLINE + 88

; Kernel is linked to run at -2Gb
KERNEL_OFFSET equ 0xFFFFFFFF80000000

; Debug builds require a larger initial boot stack,
; because their code is larger and less optimized.
%ifndef INITIAL_STACK_SIZE
%ifdef DEBUG
	INITIAL_STACK_SIZE equ 32 ; 32 pages for debug builds
%else
	INITIAL_STACK_SIZE equ 16 ; 16 pages for release builds
%endif ; DEBUG
%endif ; INITIAL_STACK_SIZE

%endif ; __DEFINES_ASM
