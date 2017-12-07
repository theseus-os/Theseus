BITS 64
section .data
    dest db 'receiver'

section .text
global  _start
_start:

    ; rax -- syscall number
    ; rdi -- first argument
    ; rsi -- second argument
    ; rdx -- third argument
    ; r10 -- fourth argument
    ; r9  -- fifth argument 
    ; r8  -- sixth argument
    
main:
    mov rax, 2 ; syscall #2 is sysrecv
    
    mov rdi, dest
    mov rsi, "default"; rsi is the pointer to the received msg
    mov rdx, 5
    mov r10, 8
    mov r8 , 13
    mov r9 , 21
    
    push rbx
    syscall    
    pop rbx
  
    mov rcx, 0x4000000
    

loopstart:
    
    ;add rax,  1

    dec rcx
    jnz loopstart
    
    mov rax, rbx

    

    ;  infinite loop
    jmp main
