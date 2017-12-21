BITS 64

section .data

src:    db "sender", 0
dest:   db "receiver", 0
msg:    db "sss", 0
; test:   dw  2

section .text
global  _start

_start:

    ; rax -- syscall number
    ; rdi -- first argument
    ; rsi -- second argument
    ; rdx -- third argument
    ; r10 -- fourth argument
    ; r8  -- fifth argument 
    ; r9  -- sixth argument
    
main:

    mov rax, 1

    ; mov rdi, "sender"    
    mov rdi, src
        
    ;mov rsi,  "receiver"
    mov rsi, dest
    
    ; mov rdx, "Hello!"
    mov rdx, msg

    mov r10, 8
    mov r8 , 13
    mov r9 , 21

    syscall
   
    ;; busy wait here for a few seconds
    mov rcx, 0x4000000
loopstart:

     ;add rax,  1

    dec rcx
    jnz loopstart
    
    mov rax, rbx

    

    ;  infinite loop
    jmp main


