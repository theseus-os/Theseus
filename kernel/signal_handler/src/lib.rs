//! Signal handlers for CPU exceptions/errors, like POSIX-style signals, but safer and Rusty.
//! 
//! Instead of directly supporting POSIX signals like SIGSEGV, SIGILL, etc,
//! this crate only supports a few categories of signals that represent a CPU/machine exception,
//! e.g., divide errors, page faults, illegal instructions, etc.
//! 
//! Each task can [register] up to one signal handler per signal kind.
//! If/when that exception occurs, the full [context] of that exception is provided as an argument
//! to the registered signal handler.
//! 
//! Signal handlers can only be invoked once. If an exception occurs, it is up to the task logic
//! to re-register that signal handler again. 
//! 
//! [register]: register_signal_handler
//! [context]: SignalContext

#![no_std]
#![feature(trait_alias)]

extern crate alloc;


use alloc::boxed::Box;
use core::cell::RefCell;
use memory::VirtualAddress;
use x86_64::structures::idt::PageFaultErrorCode;
use thread_local_macro::thread_local;


thread_local!{
    /// The signal handlers registered for the current task.
    static SIGNAL_HANDLERS: [RefCell<Option<Box<dyn SignalHandler>>>; NUM_SIGNALS] = Default::default();
}


/// Register a [`SignalHandler`] callback function for the current task.
/// 
/// If an exception/error occurs during the execution of the current task,
/// the given `handler` will be invoked with details of that exception.
/// 
/// # Return
/// * `Ok` if the signal handler was registered successfully.
/// * `Err` if a handler was already registered for the given `signal`.
pub fn register_signal_handler(
    signal: Signal,
    handler: Box<dyn SignalHandler>,
) -> Result<(), AlreadyRegistered> {
    SIGNAL_HANDLERS.with(|sig_handlers| {
        let handler_slot = &sig_handlers[signal as usize];
        if handler_slot.borrow().is_some() {
            return Err(AlreadyRegistered);
        }
        *handler_slot.borrow_mut() = Some(handler);
        Ok(())
    })
}

/// An error type indicating a handler had already been registered
/// for a particular [`Signal`].
#[derive(Debug)]
pub struct AlreadyRegistered;


/// Take the [`SignalHandler`] registered for the given `signal` for the current task.
/// 
/// This **removes** the signal handler registered for this `signal` for the current task.
/// Thus, if another exception occurs that triggers this `signal`, 
/// the returned handler will no longer exist to be invoked.
/// You'd need to re-register another handler for it using [`register_signal_handler`].
pub fn take_signal_handler(signal: Signal) -> Option<Box<dyn SignalHandler>> {
    SIGNAL_HANDLERS.with(|sig_handlers| {
        sig_handlers[signal as usize].borrow_mut().take()
    })
}


/// A signal handler is a callback function that will be invoked
/// when a task's execution causes an illegal error or exception.
/// 
/// Returning `Ok` indicates the signal was handled and that the task may continue exection.
/// Returning `Err` indicates it was not handled and that the system should proceed
/// to its default procedure of cleaning up that task.
pub trait SignalHandler = FnOnce(&SignalContext) -> Result<(), ()>;


/// The possible signals that may occur due to CPU exceptions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Signal {
    /// Bad virtual address, unexpected page fault.
    /// Analogous to SIGSEGV.
    InvalidAddress                  = 0,
    /// Invalid opcode, malformed instruction, etc.
    /// Analogous to SIGILL.
    IllegalInstruction              = 1,
    /// Bad memory alignment, non-existent physical address.
    /// Analogous to SIGBUS.
    BusError                        = 2,
    /// Bad arithmetic operation, e.g., divide by zero.
    /// Analogous to SIGFPE.
    ArithmeticError                 = 3,
    //
    // Note: if other signals are added, update `NUM_SIGNALS` below.
    //
}
const NUM_SIGNALS: usize = 4;


/// Information that is passed to a registered [`SignalHandler`]
/// about an exception that occurred during execution.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SignalContext {
    pub instruction_pointer: VirtualAddress,
    pub stack_pointer: VirtualAddress,
    pub signal: Signal,
    pub error_code: Option<ErrorCode>,
}

/// Possible error codes that may be provided by the CPU during an exception.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorCode {
    PageFaultError {
        accessed_address: usize,
        pf_error: PageFaultErrorCode,
    },
    Other(u64),
}
impl From<u64> for ErrorCode {
    fn from(other_error: u64) -> Self {
        Self::Other(other_error)
    }
}


/*
 * Note: this is currently unused, but I've left it in here in case
 *       we wish to allow tasks to register exception handlers in the future.
 *  
bitflags::bitflags! {
    /// The subset of machine exceptions that may occur on x86
    /// that Theseus allows tasks to register callback handlers for.
    pub struct Exceptions: u32 {
        const DIVIDE_ERROR                  = 1 <<  0;
        const OVERFLOW                      = 1 <<  1;
        const BOUND_RANGE_EXCEEDED          = 1 <<  2;
        const INVALID_OPCODE                = 1 <<  3;
        const DEVICE_NOT_AVAILABLE          = 1 <<  4;
        const DOUBLE_FAULT                  = 1 <<  5;
        const INVALID_TSS                   = 1 <<  6;
        const SEGMENT_NOT_PRESENT           = 1 <<  7;
        const STACK_SEGMENT_FAULT           = 1 <<  8;
        const GENERAL_PROTECTION_FAULT      = 1 <<  9;
        const PAGE_FAULT                    = 1 << 10;
        const X87_FLOATING_POINT            = 1 << 11;
        const ALIGNMENT_CHECK               = 1 << 12;
        const SIMD_FLOATING_POINT           = 1 << 13;
        //
        // Note: items below here are either reserved exceptions, or exceptions
        //       that shouldn't ever be forwarded to other tasks for handling.
        //
        // const DEBUG                         = 1 << 0;
        // const NON_MASKABLE_INTERRUPT        = 1 << 0;
        // const BREAKPOINT                    = 1 << 0;
        // reserved: 0x09
        // reserved: 0x0F
        // const MACHINE_CHECK                 = 1 << 0;
        // const VIRTUALIZATION                = 1 << 0;
        // reserved: 0x15 - 0x1C
        // const VMM_COMMUNICATION_EXCEPTION   = 1 << 0;
        // const SECURITY_EXCEPTION            = 1 << 0;
        // reserved: 0x1F
    }
}
*/
