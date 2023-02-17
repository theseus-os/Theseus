//! CPU Interface, GICv2 style
//!
//! Included functionnality:
//! - Initializing the CPU interface
//! - Setting and getting the minimum interrupt priority
//! - Acknowledging interrupt requests
//! - Sending End-Of-Interrupts signals

use super::GicMappedPage;
use super::U32BYTES;
use super::Priority;
use super::IntNumber;

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

/// Enables routing of group 1 interrupts
/// in the for the current CPU
pub fn init(registers: &mut GicMappedPage) {
    let mut reg = registers.read_volatile(offset::CTLR);
    reg |= CTLR_ENGRP1;
    registers.write_volatile(offset::CTLR, reg);
}

/// Interrupts have a priority; if their priority
/// is lower or equal to this one, they're queued
/// until this CPU or another one is ready to handle
/// them
pub fn get_minimum_priority(registers: &GicMappedPage) -> Priority {
    u8::MAX - (registers.read_volatile(offset::PMR) as u8)
}

/// Interrupts have a priority; if their priority
/// is lower or equal to this one, they're queued
/// until this CPU or another one is ready to handle
/// them
pub fn set_minimum_priority(registers: &mut GicMappedPage, priority: Priority) {
    registers.write_volatile(offset::PMR, (u8::MAX - priority) as u32);
}

/// Zeros the current priority level of this CPU,
/// Meaning that the CPU is ready to process interrupts
/// again.
pub fn end_of_interrupt(registers: &mut GicMappedPage, int: IntNumber) {
    registers.write_volatile(offset::EOIR, int as u32);
}

/// Acknowledge the currently serviced interrupt
/// and fetches its number; this tells the GIC that
/// the requested interrupt is being handled by
/// this CPU.
pub fn acknowledge_interrupt(registers: &mut GicMappedPage) -> (IntNumber, Priority) {
    // Reading the interrupt number has the side effect
    // of acknowledging the interrupt.
    let int_num = registers.read_volatile(offset::IAR) as IntNumber;
    let priority = registers.read_volatile(offset::RPR) as u8;

    (int_num, priority)
}
