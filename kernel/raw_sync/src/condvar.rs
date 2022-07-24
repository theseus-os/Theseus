use crate::Mutex;
use alloc::collections::VecDeque;
use task::TaskRef;

/// A condition variable.
///
/// The implementation is based on [a UCSD lecture][lecture].
///
/// [lecture]: https://cseweb.ucsd.edu/classes/sp17/cse120-a/applications/ln/lecture7.html
#[derive(Debug)]
pub struct Condvar {
    queue: spin::Mutex<VecDeque<&'static TaskRef>>,
    /// Ensures the mutex unlocking and thread blocking are done atomically.
    atomic_unlock_and_block: spin::Mutex<()>,
}

impl Default for Condvar {
    fn default() -> Self {
        Self {
            queue: spin::Mutex::new(VecDeque::new()),
            atomic_unlock_and_block: spin::Mutex::new(()),
        }
    }
}

impl Condvar {
    // TODO: Make const.
    pub fn new() -> Self {
        Self {
            queue: spin::Mutex::new(VecDeque::new()),
            atomic_unlock_and_block: spin::Mutex::new(()),
        }
    }

    pub fn notify_one(&self) -> bool {
        let mut queue = self.queue.lock();
        // We have to take the lock here to ensure no other thread is in the middle of
        // unlocking a mutex and blocking the thread.
        let _lock = self.atomic_unlock_and_block.lock();
        queue.pop_front().inspect(|task| task.unblock()).is_some()
    }

    pub fn notify_all(&self) {
        let mut queue = self.queue.lock();
        // We have to take the lock here to ensure no other thread is in the middle of
        // unlocking a mutex and blocking the thread.
        let _lock = self.atomic_unlock_and_block.lock();

        for task in queue.drain(..) {
            task.unblock();
        }
    }

    /// Waits for a signal on the specified mutex.
    ///
    /// # Safety
    ///
    /// Behavior is undefined if the mutex is not locked by the current thread.
    pub unsafe fn wait(&self, mutex: &Mutex) {
        let current_task = task::get_my_current_task().unwrap();

        let mut queue = self.queue.lock();
        queue.push_back(current_task);
        drop(queue);

        let atomic_unlock_and_block = self.atomic_unlock_and_block.lock();
        // SAFETY: Safety guaranteed by caller.
        unsafe { mutex.unlock() };
        current_task.block();
        drop(atomic_unlock_and_block);

        scheduler::schedule();

        // NOTE: We only reach here after the thread has been unblocked by another
        // thread.
        mutex.lock();
    }

    /// Waits for a signal on the specified mutex with a timeout duration
    /// specified by `dur` (a relative time into the future).
    ///
    /// # Safety
    ///
    /// Behavior is undefined if the mutex is not locked by the current thread.
    pub unsafe fn wait_timeout(&self, _mutex: &Mutex, _dur: core::time::Duration) -> bool {
        todo!();
    }

    /// Wait on a [`spin::Mutex`].
    ///
    /// # Safety
    ///
    /// The given `guard` must correspond to the the given `mutex`.
    pub(crate) unsafe fn wait_spin<'a, 'b, T>(
        &self,
        mutex: &'b spin::Mutex<T>,
        guard: spin::MutexGuard<'a, T>,
    ) -> spin::MutexGuard<'b, T> {
        let current_task = task::get_my_current_task().unwrap();

        let mut queue = self.queue.lock();
        queue.push_back(current_task);
        drop(queue);

        let atomic_unlock_and_block = self.atomic_unlock_and_block.lock();
        // Unlock the mutex.
        drop(guard);
        current_task.block();
        drop(atomic_unlock_and_block);

        scheduler::schedule();

        // NOTE: We only reach here after the thread has been unblocked by another
        // thread.
        mutex.lock()
    }
}
