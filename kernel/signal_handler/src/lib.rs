//! Signal handlers reminiscent of POSIX-style signals or traps, but safer and Rusty.
//! 
//! Instead of directly supporting POSIX signals like SIGSEGV, SIGBUS, etc,
//! signals here are based on types of machine exceptions that can occur, e.g.,
//! divide errors, page faults, general protection faults, etc.
//! 
//! A task can register Signal handlers
//! In addition, full context of the exception is provided as an argument
//! when a registered signal
//! 
//! 

#![no_std]
#![feature(type_alias_impl_trait)]

use memory::VirtualAddress;

bitflags::bitflags! {
    /// The set of possible signals that can occur,
    /// each of which a given signal handler can choose to handle.
    pub struct Signals: u64 {
        /// Bad virtual address, unexpected page fault.
        const INVALID_ADDRESS                  = 1 << 0;
        /// Invalid opcode, malformed instruction, etc.
        const ILLEGAL_INSTRUCTION              = 1 << 1;
        /// Bad memory alignment, non-existent physical address.
        const BUS_ERROR                        = 1 << 2;
        /// Bad arithmetic operation, e.g., divide by zero.
        const ARITHMETIC_ERROR                 = 1 << 3;
        /// Bad floating point arithmetic operation.
        const FPE_ERROR                        = 1 << 4;
    }
}


/// A given task can only register a single 
pub fn register_signal_handler<R>(exception_set: Exceptions, handler: SignalCallback<R>) -> Result<(), ()> {
    unimplemented!()
}

/// Errors that a registered signal callback 
pub enum SignalHandlerError {
    /// The signal wasn't handled by choice of the registered handler.
    NotHandled,

    /// A catch-all for other unspecified errors.
    Other(&'static str),
}


pub struct SignalContext {
    pub instruction_pointer: VirtualAddress,
    pub stack_pointer: VirtualAddress,
    pub exception: Exceptions,
}


pub type SignalCallback<R> = impl FnOnce(&SignalContext) -> Result<R, SignalHandlerError>;

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