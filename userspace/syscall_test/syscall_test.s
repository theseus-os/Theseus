BITS 64

section .text

    ; rax -- syscall number
    ; rdi -- first argument
    ; rsi -- second argument
    ; rdx -- third argument
    ; r10 -- fourth argument
    ; r8  -- fifth argument 
    ; r9  -- sixth argument
    
    mov rbx, 1; 1 is the syscall send
main:
    mov rax, rbx ; rbx is holding ground/accumulator for syscall num
    mov rdi, 10
    mov rsi, 20
    mov rdx, 30
    mov r10, 40
    mov r8 , 50
    mov r9 , 60

    push rbx
    syscall

    pop rbx
    
    mov rcx, 0x4000000
    

loopstart:
    
    dec rcx
    jnz loopstart


    ;  infinite loop
    jmp main
