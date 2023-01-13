#![no_std]

use mpmc_queue::Queue;
use sync::DeadlockPrevention;
use sync_spin::Spin;
use task::{get_my_current_task, TaskRef};

pub struct WaitQueue<P = Spin>
where
    P: DeadlockPrevention,
{
    inner: Queue<P, TaskRef>,
}

impl<P> WaitQueue<P>
where
    P: DeadlockPrevention,
{
    pub const fn new() -> Self {
        Self {
            inner: Queue::new(),
        }
    }

    pub fn wait_until<F, T>(&self, mut condition: F) -> T
    where
        F: FnMut() -> Option<T>,
    {
        loop {
            let task = get_my_current_task().unwrap();
            let wrapped_condition = || {
                if let Some(value) = condition() {
                    Some(value)
                } else {
                    // TODO: with_current_task?
                    task.block().unwrap();
                    None
                }
            };
            if let Some(value) = self.inner.push_if_fail(task.clone(), wrapped_condition) {
                return value;
            }
            scheduler::schedule();
        }
    }

    pub fn notify_one(&self) -> bool {
        // FIXME: Do we need to hold lock on queue while unblocking?
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

    pub fn notify_all(&self) {
        while self.notify_one() {}
    }
}
