BITS 64

section .text

global  _start
_start:
main:
    mov rax, 1
    mov r12, 0xDEADBEEF01234567
    mov r14, 0x0123456789abcdef

    mov rcx, 0x1000000

loopstart:
    
    add rax,  2

    dec rcx
    dec rcx
    dec rcx
    dec rcx
    jnz loopstart


    ;  infinite loop
    jmp main
