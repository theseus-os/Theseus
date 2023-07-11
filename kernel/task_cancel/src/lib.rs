// TODO: Move into task crate. Hasn't yet been done because it creates a
// circular dependency as unwind depends on task.

#![feature(abi_x86_interrupt, try_blocks)]
#![no_std]

use core::sync::atomic::Ordering;
use task::{KillReason, TaskRef};
use x86_64::structures::idt::InterruptStackFrame;

pub fn set_trap_flag(stack_frame: &mut InterruptStackFrame) {
    unsafe { stack_frame.as_mut() }.update(|stack_frame| stack_frame.cpu_flags |= 0x100);
}

pub extern "x86-interrupt" fn interrupt_handler(mut stack_frame: InterruptStackFrame) {
    let instruction_pointer = stack_frame.instruction_pointer.as_u64();
    let stack_pointer = stack_frame.stack_pointer.as_u64();

    if unwind::can_unwind(instruction_pointer) {
        log::info!("unwinding a cancelled task");
        unwind::start_remote_unwinding(
            KillReason::Requested,
            0,
            stack_pointer,
            instruction_pointer,
        )
        .expect("failed to unwind");
    } else {
        log::debug!("couldn't unwind at {instruction_pointer:0x?}; resetting trap flag");
        // The trap flag is reset after every debug interrupt. Since we can't unwind at
        // this instruction, we reset the flag to check again at the next instruction.
        set_trap_flag(&mut stack_frame);
        // FIXME: What happens if a LAPIC timer interrupt triggers here?
    }
}
