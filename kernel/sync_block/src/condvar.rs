use crate::MutexGuard;
use preemption::hold_preemption_no_timer_disable;
use mpmc_queue::Queue;
use sync::DeadlockPrevention;
use sync_spin::Spin;
use task::{get_my_current_task, TaskRef};

/// A condition variable.
///
/// Condition variables represent the ability to block a thread such that it
/// consumes no CPU time while waiting for an event to occur.
// TODO: Is there even a point to exposing this generic?
pub struct Condvar<P = Spin>
where
    P: DeadlockPrevention,
{
    inner: Queue<TaskRef, P>,
}

impl<P> Condvar<P>
where
    P: DeadlockPrevention,
{
    /// Returns a new condition variable.
    pub const fn new() -> Self {
        Self {
            inner: Queue::new(),
        }
    }

    /// Blocks the current thread until this condition variable receives a
    /// notification.
    ///
    /// Note that this function is susceptible to spurious wakeups. Condition
    /// variables normally have a boolean predicate associated with them, and
    /// the predicate must always be checked each time this function returns to
    /// protect against spurious wakeups.
    pub fn wait<'a, T: ?Sized>(&self, guard: MutexGuard<'a, T>) -> MutexGuard<'a, T> {
        let task = get_my_current_task().unwrap();
        let mutex = guard.mutex();

        let preemption_guard = self
            .inner
            .push_if_fail(task.clone(), || {
                drop(guard);
                let preemption_guard = hold_preemption_no_timer_disable();
                task.block().unwrap();
                Result::<(), _>::Err(preemption_guard)
            })
            .unwrap_err();
        drop(preemption_guard);

        loop {
            scheduler::schedule();

            match self.inner.push_if_fail(task.clone(), || {
                if let Some(mutex_guard) = mutex.try_lock() {
                    Ok(mutex_guard)
                } else {
                    let preemption_guard = hold_preemption_no_timer_disable();
                    task.block().unwrap();
                    Err(preemption_guard)
                }
            }) {
                Ok(mutex_guard) => return mutex_guard,
                Err(preemption_guard) => {
                    drop(preemption_guard);
                }
            }
        }
    }

    /// Blocks the current thread until this condition variable receives a
    /// notification and the provided condition is false.
    pub fn wait_while<'a, T, F>(
        &self,
        mut guard: MutexGuard<'a, T>,
        mut condition: F,
    ) -> MutexGuard<'a, T>
    where
        F: FnMut(&mut T) -> bool,
    {
        while condition(&mut *guard) {
            guard = self.wait(guard);
        }
        guard
    }

    fn notify_one_inner(&self) -> bool {
        loop {
            let task = match self.inner.pop() {
                Some(task) => task,
                None => return false,
            };

            if task.unblock().is_ok() {
                return true;
            }
        }
    }

    /// Wakes up one thread blocked on this condvar.
    pub fn notify_one(&self) {
        self.notify_one_inner();
    }

    /// Wakes up all threads blocked on this condvar.
    pub fn notify_all(&self) {
        while self.notify_one_inner() {}
    }
}
