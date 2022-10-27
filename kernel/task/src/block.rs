use super::*;

pub struct BlockGuard {
    task: TaskRef,
}

impl BlockGuard {
    pub(crate) fn new(task: TaskRef) -> Self {
        Self {
            task
        }
    }

    #[must_use]
    pub fn drop(self) -> bool {
        use RunState::{Blocked, Runnable};

        if self.task.runstate.compare_exchange(Blocked, Runnable).is_ok() {
            true
        } else if self.task.runstate.compare_exchange(Runnable, Runnable).is_ok() {
            warn!("Task::unblock(): unblocked an already unblocked task");
            true
        } else {
            false
        }
    }
}

