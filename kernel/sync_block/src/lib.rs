#![no_std]

use sync::{mutex, Flavour};
use sync_spin::Spin;
use wait_queue::WaitQueue;

pub struct Block {}

unsafe impl Flavour for Block {
    #[allow(clippy::declare_interior_mutable_const)]
    const INIT: Self::LockData = WaitQueue::new();

    type LockData = WaitQueue<Spin>;

    type GuardMarker = sync::GuardNoSend;

    #[inline]
    fn mutex_lock(mutex: &mutex::RawMutex<Self>) {
        if !mutex.try_lock_weak() {
            mutex.data.wait_until(|| {
                if mutex.try_lock_weak() {
                    Some(())
                } else {
                    None
                }
            });
        }
    }

    #[inline]
    fn post_unlock(mutex: &mutex::RawMutex<Self>) {
        mutex.data.notify_one();
    }
}
