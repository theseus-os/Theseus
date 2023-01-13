use crate::{mutex, Flavour, WaitQueue};

pub struct Block {}

impl crate::Sealed for Block {}

unsafe impl Flavour for Block {
    const INIT: Self::LockData = WaitQueue::new();

    type LockData = WaitQueue;

    // FIXME: Check.
    type GuardMarker = lock_api::GuardSend;

    #[inline]
    fn mutex_slow_path(mutex: &mutex::RawMutex<Self>) {
        let _ = &mutex.data;
        todo!()
    }

    #[inline]
    fn post_unlock(mutex: &mutex::RawMutex<Self>) {
        let _ = &mutex.data;
        todo!();
    }
}
