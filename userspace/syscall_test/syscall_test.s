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
    
<<<<<<< HEAD
<<<<<<< HEAD
    mov rsi,  src
    ;mov rsi, 3


    mov ax, [test]
    mov rdx, "1234567890";


=======
    ;mov rdx, [msg]
    mov rdx, "Hello!"
>>>>>>> send message be by string
=======
    ; mov rdx, "Hello!"
    mov rdx, msg
>>>>>>> ELF loading is initially working, testing with .text and .data sections. Things aren't yet cleaned up though.

    mov r10, 8
    mov r8 , 13
    mov r9 , 21

    syscall


loopstart:

    ;  infinite loop
    jmp loopstart


