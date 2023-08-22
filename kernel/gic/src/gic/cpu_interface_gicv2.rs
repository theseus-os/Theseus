//! CPU Interface, GICv2 style
//!
//! Included functionality:
//! - Initializing the CPU interface
//! - Setting and getting the minimum interrupt priority
//! - Acknowledging interrupt requests
//! - Sending End-Of-Interrupts signals

use super::Priority;
use super::InterruptNumber;

use volatile::{Volatile, ReadOnly, WriteOnly};
use zerocopy::FromBytes;

/// The GICv2 MMIO registers for interfacing with a specific CPU.
///
/// Methods herein apply to the "current" CPU only, i.e., the CPU
/// on which the code that accesses these registers is currently running.
///
/// Note: the physical address for this structure is the same for all CPUs,
/// but the actual backing memory refers to physically separate registers.
#[derive(FromBytes)]
#[repr(C)]
pub struct CpuRegsP1 {            // base offset
    /// CPU Interface Control Register
    ctlr:         Volatile<u32>,  // 0x00

    /// Interrupt Priority Mask Register
    prio_mask:    Volatile<u32>,  // 0x04

    /// Binary Point Register
    _unused0:     u32,

    /// Interrupt Acknowledge Register
    acknowledge:  ReadOnly<u32>,  // 0x0C

    /// End of Interrupt Register
    eoi:          WriteOnly<u32>, // 0x10

    /// Running Priority Register
    running_prio: ReadOnly<u32>,  // 0x14
}

// enable group 0
// const CTLR_ENGRP0: u32 = 0b01;

// enable group 1
const CTLR_ENGRP1: u32 = 0b10;

impl CpuRegsP1 {
    /// Enables routing of group 1 interrupts for the current CPU.
    pub fn init(&mut self) {
        let mut reg = self.ctlr.read();
        reg |= CTLR_ENGRP1;
        self.ctlr.write(reg);
    }

    /// Retrieves the current priority threshold for the current CPU.
    ///
    /// Interrupts have a priority; if their priority is lower or
    /// equal to this threshold, they're queued until the current CPU
    /// is ready to handle them.
    pub fn get_minimum_priority(&self) -> Priority {
        u8::MAX - (self.prio_mask.read() as u8)
    }

    /// Sets the current priority threshold for the current CPU.
    ///
    /// Interrupts have a priority; if their priority is lower or
    /// equal to this threshold, they're queued until the current CPU
    /// is ready to handle them.
    pub fn set_minimum_priority(&mut self, priority: Priority) {
        self.prio_mask.write((u8::MAX - priority) as u32);
    }

    /// Signals to the controller that the currently processed interrupt
    /// has been fully handled, by zeroing the current priority level of
    /// the current CPU.
    ///
    /// This implies that the CPU is ready to process interrupts again.
    pub fn end_of_interrupt(&mut self, int: InterruptNumber) {
        self.eoi.write(int);
    }

    /// Acknowledge the currently serviced interrupt and fetches its
    /// number.
    ///
    /// This tells the GIC that the requested interrupt is being
    /// handled by this CPU.
    pub fn acknowledge_interrupt(&mut self) -> (InterruptNumber, Priority) {
        // Reading the interrupt number has the side effect
        // of acknowledging the interrupt.
        let int_num = self.acknowledge.read() as InterruptNumber;
        let priority = self.running_prio.read() as u8;

        (int_num, priority)
    }
}
