#![no_std]

mod block;
mod mutex;
mod prevention;
mod wait_queue;

pub use block::Block;
pub use prevention::{IrqSafe, PreemptionSafe, Spin};
pub use wait_queue::WaitQueue;

pub unsafe trait Flavour {
    const INIT: Self::LockData;

    type LockData;

    type GuardMarker;

    fn mutex_slow_path(mutex: &mutex::RawMutex<Self>)
    where
        Self: Sized;

    fn post_unlock(mutex: &mutex::RawMutex<Self>)
    where
        Self: Sized;
}
