BITS 64

section .text

main:
    ; set eax to some distinguishable number, to read from the log afterwards
    mov r12, 0xDEADBEEF01234567
    mov r14, 0x0123456789abcdef

    ;  infinite loop
    jmp $
