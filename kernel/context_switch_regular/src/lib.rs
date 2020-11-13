//! This crate contains structures and routines for context switching 
//! when SSE/SIMD extensions are not active. 

#![no_std]
#![feature(llvm_asm, naked_functions)]

extern crate zerocopy;

use zerocopy::FromBytes;

/// The registers saved before a context switch and restored after a context switch.
#[derive(FromBytes)]
#[repr(C, packed)]
pub struct ContextRegular {
    // The order of the registers here MUST MATCH the order of 
    // registers popped in the restore_registers_regular!() macro below. 
    r15: usize, 
    r14: usize,
    r13: usize,
    r12: usize,
    rbp: usize,
    rbx: usize,
    rip: usize,
}

impl ContextRegular {
    /// Creates a new ContextRegular struct that will cause the
    /// Task containing it to begin its execution at the given `rip`.
    pub fn new(rip: usize) -> ContextRegular {
        ContextRegular {
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


/// An assembly macro for saving regular x86_64 registers.
/// by pushing them onto the stack.
#[macro_export]
macro_rules! save_registers_regular {
    () => (
        llvm_asm!("
            # save all general purpose registers into the previous task
            push rbx
            push rbp
            push r12
            push r13
            push r14
            push r15
            "
            : : : "memory" : "intel", "volatile"
        );
    );
}


/// An assembly macro for switching stacks,
/// which is the integral part of the actual context switching routine.
/// 
/// * The `rdi` register must contain a pointer to the previous task's stack pointer.
/// * The `rsi` register must contain the value of the next task's stack pointer.
#[macro_export]
macro_rules! switch_stacks {
    () => (
        llvm_asm!("
            # switch the stack pointers
            mov [rdi], rsp
            mov rsp, rsi
            "
            : : : "memory" : "intel", "volatile"
        );
    );
}


/// An assembly macro for saving regular x86_64 registers.
/// by pushing them onto the stack.
#[macro_export]
macro_rules! restore_registers_regular {
    () => (
        llvm_asm!("
            # restore the next task's general purpose registers
            pop r15
            pop r14
            pop r13
            pop r12
            pop rbp
            pop rbx
            "
            : : : "memory" : "intel", "volatile"
        );
    );
}


/// Switches context from a regular Task to another regular Task.
/// 
/// # Arguments
/// * First argument  (in `RDI`): mutable pointer to the previous task's stack pointer
/// * Second argument (in `RSI`): the value of the next task's stack pointer
/// 
/// # Safety
/// This function is unsafe because it changes the content on both task's stacks. 
#[naked]
#[inline(never)]
pub unsafe fn context_switch_regular(_prev_stack_pointer: *mut usize, _next_stack_pointer_value: usize) {
    // Since this is a naked function that expects its arguments in two registers,
    // you CANNOT place any log statements or other instructions here,
    // or at any point before, in between, or after the following macros.
    save_registers_regular!();
    switch_stacks!();
    restore_registers_regular!();
}
