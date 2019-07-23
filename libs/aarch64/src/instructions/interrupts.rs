//! Enable and disable hardware interrupts.
use super::super::cortex_m::interrupt;

/// Enable hardware interrupts using the `sti` instruction.
pub unsafe fn enable() {
    interrupt::enable();
}

/// Disable hardware interrupts using the `cli` instruction.
pub unsafe fn disable() {
    interrupt::disable();
}