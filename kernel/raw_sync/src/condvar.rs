use crate::Mutex;
use alloc::collections::VecDeque;
use task::TaskRef;

#[derive(Debug)]
pub struct Condvar {
    queue: spin::Mutex<VecDeque<&'static TaskRef>>,
    mutex_lock: spin::Mutex<()>,
}

impl Default for Condvar {
    fn default() -> Self {
        Self {
            queue: spin::Mutex::new(VecDeque::new()),
            mutex_lock: spin::Mutex::new(()),
        }
    }
}

impl Condvar {
    // TODO: Make const.
    pub fn new() -> Self {
        Self {
            queue: spin::Mutex::new(VecDeque::new()),
            mutex_lock: spin::Mutex::new(()),
        }
    }

    pub fn notify_one(&self) -> bool {
        let mut queue = self.queue.lock();
        let _lock = self.mutex_lock.lock();
        queue.pop_front().inspect(|task| task.unblock()).is_some()
    }

    pub fn notify_all(&self) {
        let mut queue = self.queue.lock();
        let _lock = self.mutex_lock.lock();

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

        let lock = self.mutex_lock.lock();
        // SAFETY: Safety guaranteed by caller.
        unsafe { mutex.unlock() };
        current_task.block();
        drop(lock);
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

        let lock = self.mutex_lock.lock();
        // Unlock the mutex.
        drop(guard);
        current_task.block();
        drop(lock);
        scheduler::schedule();

        // NOTE: We only reach here after the thread has been unblocked by another
        // thread.
        mutex.lock()
    }
}
