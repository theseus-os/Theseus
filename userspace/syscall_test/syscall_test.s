BITS 64

section .text

    ; rax -- syscall number
    ; rdi -- first argument
    ; rsi -- second argument
    ; rdx -- third argument
    ; r10 -- fourth argument
    ; r9  -- fifth argument 
    ; r8  -- sixth argument
    
    mov rbx, 0

main:
    mov rax, rbx ; rbx is holding ground/accumulator for syscall num
    mov rdi, 10
    mov rsi, 20
    mov rdx, 30
    mov r10, 40
    mov r9 , 50
    mov r8 , 60

    push rbx
    syscall

    pop rbx
    add rbx, 1 ; syscall num increments each time for easy tracking
    mov rcx, 0x40000000
    ; mov rcx, 0x400000

loopstart:
    
    add rax,  1

    dec rcx
    jnz loopstart


    ;  infinite loop
    jmp main
