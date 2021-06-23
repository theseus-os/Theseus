//! This crate contains structures and routines for context switching on ARM.

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
    lr: usize,   // return address to return from `context_switch()`
    r4: usize,   // the registers below are callee-saved in extern "C" calling convention
    r5: usize,
    r6: usize,
    r7: usize,
    r8: usize,
    r9: usize,
    r10: usize,
    r11: usize,
    pc: usize
}

impl ContextRegular {
    /// Creates a new ContextRegular struct that will cause the
    /// Task containing it to begin its execution at the given `pc`.
    pub fn new(pc: usize) -> ContextRegular {
        ContextRegular {
            lr: 0,
            r4: 0,
            r5: 0,
            r6: 0,
            r7: 0,
            r8: 0,
            r9: 0,
            r10: 0,
            r11: 0,
            pc
        }
    }
}


/// An assembly macro for saving ARM registers.
/// by pushing them onto the stack.
#[macro_export]
macro_rules! save_registers_regular {
    () => (
        llvm_asm!("
            # save all general purpose registers into the previous task
            push {r4-r11}
            push {lr}
            "
            : : : "memory" : "volatile"
        );
    );
}


/// An assembly macro for switching stacks,
/// which is the integral part of the actual context switching routine.
/// 
/// * The `r0` register must contain a pointer to the previous task's stack pointer.
/// * The `r1` register must contain the value of the next task's stack pointer.
#[macro_export]
macro_rules! switch_stacks {
    () => (
        llvm_asm!("
            # switch the stack pointers
            str sp, [r0]
            mov sp, r1
            "
            : : : "memory" : "volatile"
        );
    );
}


/// An assembly macro for saving regular ARM registers.
/// by pushing them onto the stack.
#[macro_export]
macro_rules! restore_registers_regular {
    () => (
        llvm_asm!("
            # restore the next task's general purpose registers
            pop {lr}
            pop {r4-r11}
            "
            : : : "memory" : "volatile"
        );
    );
}


/// Switches context from a regular Task to another regular Task.
/// 
/// # Arguments
/// * First argument  (in `r0`): mutable pointer to the previous task's stack pointer
/// * Second argument (in `r1`): the value of the next task's stack pointer
/// 
/// # Safety
/// This function is unsafe because it changes the content on both task's stacks. 
#[naked]
#[inline(never)]
pub unsafe fn context_switch_armv7em(_prev_stack_pointer: *mut usize, _next_stack_pointer_value: usize) {
    // Since this is a naked function that expects its arguments in two registers,
    // you CANNOT place any log statements or other instructions here,
    // or at any point before, in between, or after the following macros.
    save_registers_regular!();
    switch_stacks!();
    restore_registers_regular!();
}
