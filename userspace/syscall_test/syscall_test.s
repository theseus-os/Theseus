BITS 64

section .text

main:
    mov rax, 39         ; syscall number
    mov rdi, 0x1234     ; first argument
    syscall

jmp $ ; infinite loop