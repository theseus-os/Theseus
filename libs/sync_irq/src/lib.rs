#![no_std]

pub type Mutex<T> = sync::Mutex<DisableIrq, T>;
pub type MutexGuard<'a, T> = sync::MutexGuard<'a, DisableIrq, T>;

#[derive(Copy, Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DisableIrq {}

impl sync::DeadlockPrevention for DisableIrq {
    type GuardMarker = sync::GuardNoSend;

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
