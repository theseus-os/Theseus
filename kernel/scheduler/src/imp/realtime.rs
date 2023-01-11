use core::ops::{Deref, DerefMut};
use mutex_preemption::RwLockPreemptWriteGuard;
use runqueue::RunQueue;

#[derive(Debug, Clone)]
pub struct TaskRef {
    pub(crate) inner: task::TaskRef,
    period: Option<usize>,
}

impl TaskRef {
    pub(crate) fn new(task: task::TaskRef) -> Self {
        Self { inner: task }
    }
}

impl Deref for TaskRef {
    type Target = task::TaskRef;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for TaskRef {
    fn deref_mut(&mut self) -> &mut task::TaskRef {
        &mut self.inner
    }
}

pub fn set_priority(_: &task::TaskRef, _: u8) {}

pub fn get_priority(_: &task::TaskRef) {}

pub fn set_periodicity(task: &TaskRef, period: usize) {}

pub(crate) fn select_next_task(
    mut run_queue: RwLockPreemptWriteGuard<'_, RunQueue<TaskRef>>,
) -> Option<TaskRef> {
    let mut idle_task_index: Option<usize> = None;
    let mut chosen_task_index: Option<usize> = None;

    for (i, t) in run_queue.iter().enumerate() {
        // we skip the idle task, and only choose it if no other tasks are runnable
        if t.inner.is_an_idle_task {
            idle_task_index = Some(i);
            continue;
        }

        // must be runnable
        if !t.inner.is_runnable() {
            continue;
        }

        // found a runnable task!
        chosen_task_index = Some(i);
        // debug!("select_next_task(): AP {} chose Task {:?}", apic_id, &*t);
        break;
    }

    // idle task is a backup iff no other task has been chosen
    chosen_task_index
        .or(idle_task_index)
        .and_then(|index| run_queue.move_to_end(index))
}
