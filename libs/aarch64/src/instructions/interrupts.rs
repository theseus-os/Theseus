//! Enable and disable hardware interrupts.

/// Enable hardware interrupts using the `sti` instruction.
pub unsafe fn enable() {
    //TODO
}

/// Disable hardware interrupts using the `cli` instruction.
pub unsafe fn disable() {
    //TODO
}

/// Generate a software interrupt.
/// This is a macro because the argument needs to be an immediate.
#[macro_export]
macro_rules! int {
    ( $x:expr ) => {
        {
            asm!("int $0" :: "N" ($x));
        }
    };
}

/// Cause a breakpoint exception by invoking the `int3` instruction.
pub fn int3() {
    //TODO
}
