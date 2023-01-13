use crate::prevention::{private::Sealed, DeadlockPrevention};

pub struct IrqSafe {}

impl Sealed for IrqSafe {}

impl Sealed for irq_safety::HeldInterrupts {}

impl DeadlockPrevention for IrqSafe {
    type Guard = irq_safety::HeldInterrupts;

    fn enter() -> Self::Guard {
        irq_safety::hold_interrupts()
    }
}
