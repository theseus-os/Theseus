#![no_std]

use mpmc_queue::Queue;
use sync::DeadlockPrevention;
use sync_spin::Spin;
use task::{get_my_current_task, TaskRef};

pub struct WaitQueue<F = Spin>
where
    F: DeadlockPrevention,
{
    inner: Queue<F, TaskRef>,
}

impl<F> WaitQueue<F>
where
    F: DeadlockPrevention,
{
    pub const fn new() -> Self {
        Self {
            inner: Queue::new(),
        }
    }

    pub fn wait_until<A, B>(&self, mut condition: A) -> B
    where
        A: FnMut() -> Option<B>,
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
