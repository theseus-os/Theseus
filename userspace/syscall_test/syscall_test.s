BITS 64

section .text

    ; rax -- syscall number
    ; rdi -- first argument
    ; rsi -- second argument
    ; rdx -- third argument
    ; r10 -- fourth argument
    ; r9  -- fifth argument 
    ; r8  -- sixth argument
    
main:
    mov rax, 0
    mov rdi, 1
    mov rsi, 2
    mov rdx, 3
    mov r10, 4
    mov r9 , 5
    mov r8 , 6

    syscall

jmp $ ; infinite loop