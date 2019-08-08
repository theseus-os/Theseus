//! This crate contains structures and routines for context switching 
//! on ARM64

#![no_std]
#![feature(asm, naked_functions)]


/// The registers saved before a context switch and restored after a context switch.
#[repr(C, packed)]
pub struct ContextARM {
    // The order of the registers here MUST MATCH the order of 
    // registers popped in the restore_registers_regular!() macro below. 
    // TODO
}

impl ContextARM {
    /// Creates a new ContextARM struct that will cause the
    /// Task containing it to begin its execution at the given `rip`.
    pub fn new(rip: usize) -> ContextARM {
        ContextARM {
            // TODO
        }
    }
}


/// An assembly macro for saving regular aarch64 registers.
/// by pushing them onto the stack.
#[macro_export]
macro_rules! save_registers_arm {
    () => (
        // TODO
    );
}


/// An assembly macro for switching stacks,
/// which is the integral part of the actual context switching routine.
#[macro_export]
macro_rules! switch_stacks {
    () => (
        // TODO
    );
}


/// An assembly macro for saving aarch64 registers.
/// by pushing them onto the stack.
#[macro_export]
macro_rules! restore_registers_arm {
    () => (
        // TODO
    );
}


/// Switches context from a arm Task to another arm Task.
/// 
#[naked]
#[inline(never)]
pub unsafe fn context_switch_arm() {
    save_registers_arm!();
    switch_stacks!();
    restore_registers_arm!();
}