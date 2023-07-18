//! A simple wrapper struct around an x86_64 Interrupt Descriptor Table (IDT).

#![no_std]

use sync_irq::{IrqSafeMutex, IrqSafeMutexGuard};
use x86_64::structures::idt::InterruptDescriptorTable;

/// A thread-safe and interrupt-safe wrapper around [`InterruptDescriptorTable`]. 
/// 
/// This type offers interior mutability, allowing interrupt handlers to be added/changed/removed,
/// but preserves safety by guaranteeing that only a static object can be loaded.
#[derive(Debug)]
pub struct LockedIdt {
    idt: IrqSafeMutex<InterruptDescriptorTable>,
}
impl LockedIdt {
    /// Creates a new IDT filled with non-present entries.
    pub const fn new() -> LockedIdt {
        LockedIdt {
            idt: IrqSafeMutex::new(InterruptDescriptorTable::new()),
        }
    }

    /// Obtains the lock on the inner IDT and loads it into the current CPU
    /// using the `lidt` command.
    pub fn load(&'static self) {
        unsafe { self.idt.lock().load_unsafe(); }
    } 

    /// Obtains the lock on the inner IDT and returns a guard that derefs into it.
    /// 
    /// Interrupts are also disabled until the guard falls out of scope,
    /// at which point they are re-enabled iff they were previously enabled
    /// when this function was invoked. 
    /// and the lock will be dropped when the guard falls out of scope.
    pub fn lock(&self) -> IrqSafeMutexGuard<InterruptDescriptorTable> {
        self.idt.lock()
    }
}
