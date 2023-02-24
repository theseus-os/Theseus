//! CPU Interface, GICv2 style
//!
//! Included functionality:
//! - Initializing the CPU interface
//! - Setting and getting the minimum interrupt priority
//! - Acknowledging interrupt requests
//! - Sending End-Of-Interrupts signals

use super::GicRegisters;
use super::U32BYTES;
use super::Priority;
use super::InterruptNumber;

mod offset {
    use super::U32BYTES;
    pub const CTLR: usize = 0x00 / U32BYTES;
    pub const PMR:  usize = 0x04 / U32BYTES;
    pub const IAR:  usize = 0x0C / U32BYTES;
    pub const RPR:  usize = 0x14 / U32BYTES;
    pub const EOIR: usize = 0x10 / U32BYTES;
}

// enable group 0
// const CTLR_ENGRP0: u32 = 0b01;

// enable group 1
const CTLR_ENGRP1: u32 = 0b10;

/// Enables routing of group 1 interrupts for the current CPU
pub fn init(registers: &mut GicRegisters) {
    let mut reg = registers.read_volatile(offset::CTLR);
    reg |= CTLR_ENGRP1;
    registers.write_volatile(offset::CTLR, reg);
}

/// Interrupts have a priority; if their priority
/// is lower or equal to this one, they're queued
/// until this CPU or another one is ready to handle
/// them
pub fn get_minimum_priority(registers: &GicRegisters) -> Priority {
    u8::MAX - (registers.read_volatile(offset::PMR) as u8)
}

/// Interrupts have a priority; if their priority
/// is lower or equal to this one, they're queued
/// until this CPU or another one is ready to handle
/// them
pub fn set_minimum_priority(registers: &mut GicRegisters, priority: Priority) {
    registers.write_volatile(offset::PMR, (u8::MAX - priority) as u32);
}

/// Signals to the controller that the currently processed interrupt has
/// been fully handled, by zeroing the current priority level of this CPU.
/// This implies that the CPU is ready to process interrupts again.
pub fn end_of_interrupt(registers: &mut GicRegisters, int: InterruptNumber) {
    registers.write_volatile(offset::EOIR, int as u32);
}

/// Acknowledge the currently serviced interrupt
/// and fetches its number; this tells the GIC that
/// the requested interrupt is being handled by
/// this CPU.
pub fn acknowledge_interrupt(registers: &mut GicRegisters) -> (InterruptNumber, Priority) {
    // Reading the interrupt number has the side effect
    // of acknowledging the interrupt.
    let int_num = registers.read_volatile(offset::IAR) as InterruptNumber;
    let priority = registers.read_volatile(offset::RPR) as u8;

    (int_num, priority)
}
