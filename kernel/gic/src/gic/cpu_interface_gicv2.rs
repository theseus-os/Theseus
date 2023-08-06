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

#[derive(FromBytes)]
#[repr(C)]
pub struct CpuRegsP1 {            // base offset
    ctlr:         Volatile<u32>,  // 0x00
    prio_mask:    Volatile<u32>,  // 0x04
    _unused0:     u32,
    acknowledge:  ReadOnly<u32>,  // 0x0C
    eoi:          WriteOnly<u32>, // 0x10
    running_prio: ReadOnly<u32>,  // 0x14
}

// enable group 0
// const CTLR_ENGRP0: u32 = 0b01;

// enable group 1
const CTLR_ENGRP1: u32 = 0b10;

/// Enables routing of group 1 interrupts for the current CPU
pub fn init(registers: &mut CpuRegsP1) {
    let mut reg = registers.ctlr.read();
    reg |= CTLR_ENGRP1;
    registers.ctlr.write(reg);
}

/// Interrupts have a priority; if their priority
/// is lower or equal to this one, they're queued
/// until this CPU or another one is ready to handle
/// them
pub fn get_minimum_priority(registers: &CpuRegsP1) -> Priority {
    u8::MAX - (registers.prio_mask.read() as u8)
}

/// Interrupts have a priority; if their priority
/// is lower or equal to this one, they're queued
/// until this CPU or another one is ready to handle
/// them
pub fn set_minimum_priority(registers: &mut CpuRegsP1, priority: Priority) {
    registers.prio_mask.write((u8::MAX - priority) as u32);
}

/// Signals to the controller that the currently processed interrupt has
/// been fully handled, by zeroing the current priority level of this CPU.
/// This implies that the CPU is ready to process interrupts again.
pub fn end_of_interrupt(registers: &mut CpuRegsP1, int: InterruptNumber) {
    registers.eoi.write(int);
}

/// Acknowledge the currently serviced interrupt
/// and fetches its number; this tells the GIC that
/// the requested interrupt is being handled by
/// this CPU.
pub fn acknowledge_interrupt(registers: &mut CpuRegsP1) -> (InterruptNumber, Priority) {
    // Reading the interrupt number has the side effect
    // of acknowledging the interrupt.
    let int_num = registers.acknowledge.read() as InterruptNumber;
    let priority = registers.running_prio.read() as u8;

    (int_num, priority)
}
