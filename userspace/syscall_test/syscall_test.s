BITS 64

section .data

    src: db 'sender', 0
    dest: db 'receiver', 0
    msg db "sss", 0
    test: dw  2

section .text

    ; rax -- syscall number
    ; rdi -- first argument
    ; rsi -- second argument
    ; rdx -- third argument
    ; r10 -- fourth argument
    ; r9  -- fifth argument 
    ; r8  -- sixth argument
    
    mov rbx, 1; 1 is the syscall send

main:

    mov rax, rbx ; rbx is holding ground/accumulator for syscall num

    mov rdi, "sender"    
        
    mov rsi,  "receiver"
    
    ;mov rdx, [msg]
    mov rdx, "Hello!"

    mov r10, 8
    mov r9 , 13
    mov r8 , 21

    push rbx
    syscall

    pop rbx
    
    mov rcx, 0x4000000
    

loopstart:
    
    dec rcx
    jnz loopstart


    ;  infinite loop
    jmp main


