use mpmc_queue::Queue;
use task::{get_my_current_task, TaskRef};

pub struct WaitQueue {
    inner: Queue<TaskRef>,
}

impl WaitQueue {
    pub const fn new() -> Self {
        Self {
            inner: Queue::new(),
        }
    }

    pub fn wait_until<F, T>(&self, condition: F) -> T
    where
        F: Fn() -> Option<T>,
    {
        loop {
            let task = get_my_current_task().unwrap();
            let wrapped_condition = || {
                if let Some(value) = condition() {
                    Some(value)
                } else {
                    task.block().unwrap();
                    None
                }
            };
            if let Some(value) = self.inner.push_if_fail(task, &wrapped_condition) {
                return value;
            }
            todo!();
            // scheduler::schedule();
        }
    }
}
