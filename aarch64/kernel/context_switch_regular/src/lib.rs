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
    x2: usize, x3: usize,
    x4: usize, x5: usize,
    x6: usize, x7: usize,
    x8: usize, x9: usize,
    x10: usize, x11: usize,
    x12: usize, x13: usize,
    x14: usize, x15: usize,
    x16: usize, x17: usize,
    x18: usize, x19: usize,
    x20: usize, x21: usize,
    x22: usize, x23: usize,
    x24: usize, x25: usize,
    x26: usize, x27: usize,
    x28: usize, x29: usize,

    // x30 stores the return address
    // it's used by the `ret` instruction
    x30: usize,
}

impl ContextRegular {
    /// Creates a new [`ContextRegular`] struct that will cause the
    /// Task containing it to begin its execution at the given `start_address`.
    pub fn new(start_address: usize) -> ContextRegular {
        ContextRegular {
            x2: 0, x3: 0,
            x4: 0, x5: 0,
            x6: 0, x7: 0,
            x8: 0, x9: 0,
            x10: 0, x11: 0,
            x12: 0, x13: 0,
            x14: 0, x15: 0,
            x16: 0, x17: 0,
            x18: 0, x19: 0,
            x20: 0, x21: 0,
            x22: 0, x23: 0,
            x24: 0, x25: 0,
            x26: 0, x27: 0,
            x28: 0, x29: 0,

            // x30 stores the return address
            // it's used by the `ret` instruction
            x30: start_address,
        }
    }

    /// Sets the value of the first register to the given `value`.
    /// 
    /// This is useful for storing a value (e.g., task ID) in that register
    /// and then recovering it later with [`read_first_register()`].
    /// 
    /// On aarch64, this sets the `x2` register.
    pub fn set_first_register(&mut self, value: usize) {
        self.x2 = value;
    }
}

/// Reads the value of the first register from the actual CPU register hardware.
/// 
/// This can be called at any time, but is intended for use as the second half
/// of "saving and restoring" a register value.
/// The first half was a previous call to [`ContextRegular::set_first_register()`],
/// and the second half is a call to this function immediately after the original
/// `ContextRegular` has been used for switching to a new task for the first time.
/// 
/// Returns the current value held in the specified CPU register.
/// On aarch64, this reads the x2 register.
#[naked]
pub extern "C" fn read_first_register() -> usize {
    unsafe {
        core::arch::asm!(
            "mov x0, x2", // x0 is the default return-value register on aarch64
            "ret",
            options(noreturn)
        )
    }
}

/// An assembly block for saving regular aarch64 registers
/// by pushing them onto the stack.
#[macro_export]
macro_rules! save_registers_regular {
    () => (
        // Save all general purpose registers into the previous task.
        r#"
            // Make room on the stack for the exception context.
            // This is 8 bytes too much, but has better alignment.
            sub sp,  sp,  #8 * 29

            // Push general-purpose registers on the stack.
            stp x2,  x3,  [sp, #8 *  0 * 2]
            stp x4,  x5,  [sp, #8 *  1 * 2]
            stp x6,  x7,  [sp, #8 *  2 * 2]
            stp x8,  x9,  [sp, #8 *  3 * 2]
            stp x10, x11, [sp, #8 *  4 * 2]
            stp x12, x13, [sp, #8 *  5 * 2]
            stp x14, x15, [sp, #8 *  6 * 2]
            stp x16, x17, [sp, #8 *  7 * 2]
            stp x18, x19, [sp, #8 *  8 * 2]
            stp x20, x21, [sp, #8 *  9 * 2]
            stp x22, x23, [sp, #8 * 10 * 2]
            stp x24, x25, [sp, #8 * 11 * 2]
            stp x26, x27, [sp, #8 * 12 * 2]
            stp x28, x29, [sp, #8 * 13 * 2]

            // x30 stores the return address.
            str x30,      [sp, #8 * 14 * 2]
        "#
    );
}

/// An assembly block for switching stacks,
/// which is the integral part of the actual context switching routine.
/// 
/// * The `x0` register must contain a pointer to the previous task's stack pointer.
/// * The `x1` register must contain the value of the next task's stack pointer.
#[macro_export]
macro_rules! switch_stacks {
    () => [
        // switch the stack pointers
        r#"
            // Save current stack pointer to address in 1st argument.
            mov x2, sp
            str x2, [x0, 0]

            // Set the stack pointer to value in 2nd argument.
            mov sp, x1
        "#
    ];
}

/// An assembly block for restoring regular aarch64 registers
/// by popping them off of the stack.
#[macro_export]
macro_rules! restore_registers_regular {
    () => (
        // Restore the next task's general purpose registers.
        r#"
            // Pop general-purpose registers from the stack.
            ldp x2,  x3,  [sp, #8 *  0 * 2]
            ldp x4,  x5,  [sp, #8 *  1 * 2]
            ldp x6,  x7,  [sp, #8 *  2 * 2]
            ldp x8,  x9,  [sp, #8 *  3 * 2]
            ldp x10, x11, [sp, #8 *  4 * 2]
            ldp x12, x13, [sp, #8 *  5 * 2]
            ldp x14, x15, [sp, #8 *  6 * 2]
            ldp x16, x17, [sp, #8 *  7 * 2]
            ldp x18, x19, [sp, #8 *  8 * 2]
            ldp x20, x21, [sp, #8 *  9 * 2]
            ldp x22, x23, [sp, #8 * 10 * 2]
            ldp x24, x25, [sp, #8 * 11 * 2]
            ldp x26, x27, [sp, #8 * 12 * 2]
            ldp x28, x29, [sp, #8 * 13 * 2]

            // x30 stores the return address.
            ldr x30,      [sp, #8 * 14 * 2]

            // Move the stack pointer back up.
            add sp,  sp,  #8 * 30
        "#
    );
}

/// Switches context from a regular Task to another regular Task.
/// 
/// # Arguments
/// * First argument  (in `x0`): mutable pointer to the previous task's stack pointer
/// * Second argument (in `x1`): the value of the next task's stack pointer
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
        "ret",
        options(noreturn)
    );
}
