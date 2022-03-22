//! This crate contains structures and routines for context switching 
//! when SSE/SIMD extensions are not active. 

#![no_std]
#![feature(naked_functions)]

extern crate zerocopy;

use zerocopy::FromBytes;

/// The registers saved before a context switch and restored after a context switch.
///
/// Note: the order of the registers here MUST MATCH the order of 
/// registers popped in the [`restore_registers_regular!`] macro. 
#[derive(FromBytes)]
#[repr(C, packed)]
pub struct ContextRegular {
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
            rip,
        }
    }
}


/// An assembly block for saving regular x86_64 registers
/// by pushing them onto the stack.
#[macro_export]
macro_rules! save_registers_regular {
    () => (
        // save all general purpose registers into the previous task
        r#"
            push rbx
            push rbp
            push r12
            push r13
            push r14
            push r15
        "#
    );
}


/// An assembly block for switching stacks,
/// which is the integral part of the actual context switching routine.
/// 
/// * The `rdi` register must contain a pointer to the previous task's stack pointer.
/// * The `rsi` register must contain the value of the next task's stack pointer.
#[macro_export]
macro_rules! switch_stacks {
    () => (
        // switch the stack pointers
        r#"
            mov [rdi], rsp
            mov rsp, rsi
        "#
    );
}


/// An assembly block for restoring regular x86_64 registers
/// by popping them off of the stack.
/// 
/// This assembly statement ends with an explicit `ret` instruction at the end,
/// which is the final component of a context switch operation. 
/// Note that this is intentional and required in order to accommodate 
/// the `noreturn` option is required by Rust's naked functions.
#[macro_export]
macro_rules! restore_registers_regular {
    () => (
        // restore the next task's general purpose registers
        r#" 
            pop r15
            pop r14
            pop r13
            pop r12
            pop rbp
            pop rbx
            ret
        "#
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
pub unsafe extern "C" fn context_switch_regular(_prev_stack_pointer: *mut usize, _next_stack_pointer_value: usize) {
    // Since this is a naked function that expects its arguments in two registers,
    // you CANNOT place any log statements or other instructions here
    // before, in between, or after anything below.
    core::arch::asm!(
        save_registers_regular!(),
        switch_stacks!(),
        restore_registers_regular!(),
        options(noreturn)
    );
}
