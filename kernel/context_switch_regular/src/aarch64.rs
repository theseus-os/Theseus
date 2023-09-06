//! This crate contains structures and routines for context switching on aarch64.

use zerocopy::{FromBytes, AsBytes};

/// The registers saved before a context switch and restored after a context switch.
///
/// Note: the order of the registers here MUST MATCH the order of 
/// registers popped in the [`restore_registers_regular!`] macro. 
#[derive(FromBytes, AsBytes)]
#[repr(C, packed)]
pub struct ContextRegular {
    x19: usize, x20: usize,
    x21: usize, x22: usize,
    x23: usize, x24: usize,
    x25: usize, x26: usize,
    x27: usize, x28: usize,
    x29_frame_register: usize,

    // x30 stores the return address
    // it's used by the `ret` instruction
    x30_link_register: usize,

    // only NZCV & DAIF bits are saved and restored
    pstate: usize,
}

impl ContextRegular {
    /// Creates a new [`ContextRegular`] struct that will cause the
    /// Task containing it to begin its execution at the given `start_address`.
    pub fn new(start_address: usize) -> ContextRegular {
        ContextRegular {
            x19: 0, x20: 0,
            x21: 0, x22: 0,
            x23: 0, x24: 0,
            x25: 0, x26: 0,
            x27: 0, x28: 0,
            x29_frame_register: 0,

            // x30 stores the return address
            // it's used by the `ret` instruction
            x30_link_register: start_address,

            // interrupts are initially unmasked/enabled
            pstate: 0,
        }
    }

    /// Sets the value of the first register to the given `value`.
    /// 
    /// This is useful for storing a value (e.g., task ID) in that register
    /// and then recovering it later with [`read_first_register()`].
    /// 
    /// On aarch64, this sets the `x28` register.
    pub fn set_first_register(&mut self, value: usize) {
        self.x28 = value;
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
/// On aarch64, this reads the x28 register.
#[naked]
pub extern "C" fn read_first_register() -> usize {
    unsafe {
        core::arch::asm!(
            "mov x0, x28", // x0 is the default return-value register on aarch64
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
            // Make room on the stack for the registers.
            sub sp,  sp,  #8 * 2 * 6

            // Push registers on the stack, two at a time.
            stp x19, x20, [sp, #8 * 2 * 0]
            stp x21, x22, [sp, #8 * 2 * 1]
            stp x23, x24, [sp, #8 * 2 * 2]
            stp x25, x26, [sp, #8 * 2 * 3]
            stp x27, x28, [sp, #8 * 2 * 4]
            stp x29, x30, [sp, #8 * 2 * 5]

            // Push an OR of DAIF and NZCV flags of PSTATE
            mrs x29, DAIF
            mrs x30, NZCV
            orr x29, x29, x30
            str x29, [sp, #8 * 2 * 6]
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
            str x2, [x0]

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
            // Pop DAIF and NZCV flags of PSTATE
            // These MSRs discard irrelevant bits; no AND is required.
            ldr x29, [sp, #8 * 2 * 6]
            msr DAIF, x29
            msr NZCV, x29

            // Pop registers from the stack, two at a time.
            ldp x29, x30, [sp, #8 * 2 * 5]
            ldp x27, x28, [sp, #8 * 2 * 4]
            ldp x25, x26, [sp, #8 * 2 * 3]
            ldp x23, x24, [sp, #8 * 2 * 2]
            ldp x21, x22, [sp, #8 * 2 * 1]
            ldp x19, x20, [sp, #8 * 2 * 0]

            // Move the stack pointer back up.
            add sp,  sp,  #8 * 2 * 6
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
