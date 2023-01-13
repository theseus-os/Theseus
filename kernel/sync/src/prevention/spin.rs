use crate::prevention::DeadlockPrevention;

pub struct Spin {}

impl DeadlockPrevention for Spin {
    type GuardMarker = lock_api::GuardSend;

    #[inline]
    fn enter() {}

    #[inline]
    fn exit() {}
}
