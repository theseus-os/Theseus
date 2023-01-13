use crate::prevention::DeadlockPrevention;

pub struct IrqSafe {}

impl DeadlockPrevention for IrqSafe {
    type GuardMarker = lock_api::GuardNoSend;

    #[inline]
    fn enter() {
        // FIXME: Recursive disabling doesn't work.
        irq_safety::disable_interrupts();
    }

    #[inline]
    fn exit() {
        irq_safety::enable_interrupts();
    }
}
