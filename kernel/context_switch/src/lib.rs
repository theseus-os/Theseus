//! This crate contains structures and routines for context switching 
//! when SSE/SIMD extensions are not active. 

#![no_std]
#![feature(asm, naked_functions)]


/// The registers saved before a context switch and restored after a context switch.
#[repr(C, packed)]
pub struct Context {
    // The order of the registers here MUST MATCH the order of 
    // registers popped in the context_switch() function below. 
    r15: usize, 
    r14: usize,
    r13: usize,
    r12: usize,
    rbp: usize,
    rbx: usize,
    rip: usize,
}

impl Context {
    pub fn new(rip: usize) -> Context {
        Context {
            r15: 0,
            r14: 0,
            r13: 0,
            r12: 0,
            rbp: 0,
            rbx: 0,
            rip: rip,
        }
    }
}


/// Performs the actual switch from the previous (current) task to the next task.
/// 
/// This is the regular task switch routine for when SSE/SIMD extensions are not being used.
/// 
/// # Arguments
/// First argument  (put in `rdi`): mutable pointer to the previous task's stack pointer
/// Second argument (put in `rsi`): the value of the next task's stack pointer
/// 
/// # Safety
/// This function is unsafe because it changes the content on both task's stacks. 
/// Also, it must be a naked function, so there cannot be regular arguments passed into it.
/// Instead, the caller of this function must place the first argument into the `rdi` register
/// and the second argument into the `rsi` register right before invoking this function.
#[allow(private_no_mangle_fns)]
#[naked]
#[no_mangle]
#[inline(never)]
pub unsafe fn context_switch() {
    
    asm!("
        # save all general purpose registers into the previous task
        push rbx
        push rbp
        push r12
        push r13
        push r14
        push r15
        
        # switch the stack pointers
        mov [rdi], rsp
        mov rsp, rsi

        # restore the next task's general purpose registers
        pop r15
        pop r14
        pop r13
        pop r12
        pop rbp
        pop rbx

        # pops the last value off the top of the stack,
        # so the new task's stack top must point to a target function
        ret"
        : : : "memory" : "intel", "volatile"
    );
}
