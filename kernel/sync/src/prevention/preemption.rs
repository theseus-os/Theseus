use crate::prevention::{private::Sealed, DeadlockPrevention};

pub struct PreemptionSafe {}

impl Sealed for PreemptionSafe {}

impl Sealed for preemption::PreemptionGuard {}

impl DeadlockPrevention for PreemptionSafe {
    type Guard = preemption::PreemptionGuard;

    fn enter() -> Self::Guard {
        preemption::hold_preemption()
    }
}
