mod irq;
mod preemption;
mod spin;

use crate::{mutex, Flavour, Sealed};
use lock_api::RawMutex;

pub use self::preemption::PreemptionSafe;
pub use self::spin::Spin;
pub use irq::IrqSafe;

pub trait DeadlockPrevention {
    type GuardMarker;

    fn enter();

    fn exit();
}

impl<T> Sealed for T where T: DeadlockPrevention {}

unsafe impl<T> Flavour for T
where
    T: DeadlockPrevention,
{
    const INIT: Self::LockData = ();

    type LockData = ();

    type GuardMarker = <Self as DeadlockPrevention>::GuardMarker;

    #[inline]
    fn mutex_slow_path(mutex: &mutex::RawMutex<Self>) {
        T::enter();
        while !mutex.try_lock_weak() {
            T::exit();
            while mutex.is_locked() {
                core::hint::spin_loop();
            }
            T::enter();
        }
    }

    #[inline]
    fn post_unlock(_: &mutex::RawMutex<Self>) {
        T::exit();
    }
}
