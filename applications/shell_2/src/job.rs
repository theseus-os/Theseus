use alloc::vec::Vec;
use task::JoinableTaskRef;

pub(crate) struct Job {
    tasks: Vec<JoinableTaskRef>,
}

impl Job {
    pub(crate) fn unblock(&self) {
        for task in self.tasks {
            task.unblock();
        }
    }
}
