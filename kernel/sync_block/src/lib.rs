#![feature(negative_impls, let_chains)]
#![no_std]

mod condvar;

use core::sync::atomic::{AtomicUsize, Ordering};
use sync::{spin, MutexFlavor, RwLockFlavor};
use wait_queue::WaitQueue;

pub use condvar::Condvar;

#[cfg(feature = "std-api")]
pub mod std_api;

pub type Mutex<T> = sync::Mutex<T, Block>;
pub type MutexGuard<'a, T> = sync::MutexGuard<'a, T, Block>;

pub type RwLock<T> = sync::RwLock<T, Block>;
pub type RwLockReadGuard<'a, T> = sync::RwLockReadGuard<'a, T, Block>;
pub type RwLockWriteGuard<'a, T> = sync::RwLockWriteGuard<'a, T, Block>;

/// A synchronisation flavour that blocks the current thread while waiting for
/// the lock to become available.
#[derive(Copy, Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Block {}

impl MutexFlavor for Block {
    #[allow(clippy::declare_interior_mutable_const)]
    const INIT: Self::LockData = Self::LockData {
        queue: WaitQueue::new(),
        holder: AtomicUsize::new(0),
    };

    type LockData = MutexData;

    type Guard = ();

    #[inline]
    fn try_lock<'a, T>(
        mutex: &'a spin::Mutex<T>,
        data: &'a Self::LockData,
    ) -> Option<(spin::MutexGuard<'a, T>, Self::Guard)>
    where
        T: ?Sized,
    {
        let guard = mutex.try_lock()?;
        // There is an interleaving where we are here, the previous holder hasn't yet
        // ran post_unlock, and another thread A acquires the holder_id for the middle
        // path all at the same time. This will result in thread A waiting on the old
        // holder of the lock. This isn't great, but it is still correct, as the thread
        // would only do so for a short period of time. It does however mean that the
        // middle path would be "useless" because as soon as the holder changes, thread
        // A would enter the slow path.
        //
        // We can prevent this by using an atomic usize in the inner spin lock rather
        // than an atomic bool. A non-zero value would represent the task ID of the
        // holder, and a zero would represent the unlocked state. However, this
        // would be very hard to integrate with the current sync API.
        data.holder
            .store(task::get_my_current_task_id(), Ordering::Release);
        Some((guard, ()))
    }

    #[inline]
    fn lock<'a, T>(
        mutex: &'a spin::Mutex<T>,
        data: &'a Self::LockData,
    ) -> (spin::MutexGuard<'a, T>, Self::Guard)
    where
        T: ?Sized,
    {
        // Fast path
        // This must be a strong compare exchange, otherwise we could block ourselves
        // when the mutex is unlocked and never be unblocked.
        if let Some(guards) = Self::try_lock(mutex, data) {
            return guards;
        }

        // Middle path: Wait for one timeslice (see below) of the mutex holder to see if
        // they release the lock.
        let holder_id = data.holder.load(Ordering::Acquire);
        if holder_id != 0 && let Some(holder_task) = task::get_task(holder_id).and_then(|task| task.upgrade()) {
            // Hypothetically, if holder_task is running on another core and is perfectly in
            // sync with us, we would only ever check if they are running when we are also
            // running and so we wouldn't detect when their timeslice is over. However, the
            // likelihood of this is infinitesimally small and the code, is still correct as
            // once the lock is released the holder will still set data.holder to 0 and we
            // will exit the loop.
            while holder_task.is_running() && data.holder.load(Ordering::Acquire) == holder_id {
                core::hint::spin_loop();
            }
            // Holder is either no longer running, or has released the lock.
            // Either way we will try the fast path one more time before moving
            // onto the slow path.

            if let Some(guards) = Self::try_lock(mutex, data) {
                return guards;
            }

            // Slow path
            #[cfg(priority_inheritance)]
            let _priority_guard = scheduler::inherit_priority(&holder_task);

            data.queue.wait_until(|| Self::try_lock(mutex, data))
        } else {
            // Unlikely case that another thread just acquired the lock, but hasn't yet set
            // data.holder.
            log::warn!("could not get holder task for mutex middle path");

            if let Some(guards) = Self::try_lock(mutex, data) {
                return guards;
            }

            // Slow path
            data.queue.wait_until(|| Self::try_lock(mutex, data))
        }
    }

    #[inline]
    fn post_unlock(data: &Self::LockData) {
        // See comments in try_lock and lock on why this is necessary.
        data.holder.store(0, Ordering::Release);
        data.queue.notify_one();
    }
}

#[doc(hidden)]
pub struct MutexData {
    queue: WaitQueue,
    holder: AtomicUsize,
}

impl RwLockFlavor for Block {
    #[allow(clippy::declare_interior_mutable_const)]
    const INIT: Self::LockData = RwLockData {
        readers: WaitQueue::new(),
        writers: WaitQueue::new(),
    };

    type LockData = RwLockData;

    type Guard = ();

    #[inline]
    fn try_read<'a, T>(
        rw_lock: &'a spin::RwLock<T>,
        _: &'a Self::LockData,
    ) -> Option<(spin::RwLockReadGuard<'a, T>, Self::Guard)>
    where
        T: ?Sized,
    {
        rw_lock.try_read().map(|guard| (guard, ()))
    }

    #[inline]
    fn try_write<'a, T>(
        rw_lock: &'a spin::RwLock<T>,
        _: &'a Self::LockData,
    ) -> Option<(spin::RwLockWriteGuard<'a, T>, Self::Guard)>
    where
        T: ?Sized,
    {
        rw_lock.try_write().map(|guard| (guard, ()))
    }

    #[inline]
    fn read<'a, T>(
        rw_lock: &'a spin::RwLock<T>,
        data: &'a Self::LockData,
    ) -> (spin::RwLockReadGuard<'a, T>, Self::Guard)
    where
        T: ?Sized,
    {
        if let Some(guards) = Self::try_read(rw_lock, data) {
            guards
        } else {
            data.readers.wait_until(|| Self::try_read(rw_lock, data))
        }
    }

    #[inline]
    fn write<'a, T>(
        rw_lock: &'a spin::RwLock<T>,
        data: &'a Self::LockData,
    ) -> (spin::RwLockWriteGuard<'a, T>, Self::Guard)
    where
        T: ?Sized,
    {
        // This must be a strong compare exchange, otherwise we could block ourselves
        // when the lock is unlocked and never be unblocked.
        if let Some(guards) = Self::try_write(rw_lock, data) {
            guards
        } else {
            data.writers.wait_until(|| Self::try_write(rw_lock, data))
        }
    }

    #[inline]
    fn post_unlock(data: &Self::LockData, is_writer_or_last_reader: bool) {
        if is_writer_or_last_reader && !data.writers.notify_one() {
            data.readers.notify_all();
        }
    }
}

#[doc(hidden)]
pub struct RwLockData {
    readers: WaitQueue,
    writers: WaitQueue,
}
