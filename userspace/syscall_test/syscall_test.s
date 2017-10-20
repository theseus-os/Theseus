BITS 64

section .data

src: db 'sender', 0
message db "sss", 0
test: dw  0abcdh

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

    mov rdi, "sender"    
        
    mov rsi,  "receiver"
    
<<<<<<< HEAD
    mov rsi,  src
    ;mov rsi, 3


    mov ax, [test]
    mov rdx, "1234567890";


=======
    ;mov rdx, [msg]
    mov rdx, "Hello!"
>>>>>>> send message be by string

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


