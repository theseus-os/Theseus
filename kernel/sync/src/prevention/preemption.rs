use crate::prevention::DeadlockPrevention;

pub struct PreemptionSafe {}

impl DeadlockPrevention for PreemptionSafe {
    type GuardMarker = lock_api::GuardNoSend;

    #[inline]
    fn enter() {
        preemption::enable_preemption()
    }

    #[inline]
    fn exit() {
        preemption::disable_preemption()
    }
}
