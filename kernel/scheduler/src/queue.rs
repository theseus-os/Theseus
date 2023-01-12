use crate::imp;
use task::TaskRef;

#[derive(Debug)]
pub struct RunQueue {
    #[cfg_attr(not(single_simd_task_optimisation), allow(dead_code))]
    core: u8,
    idle_task: TaskRef,
    queue: imp::Queue,
}

impl RunQueue {
    pub(crate) fn new(core: u8, idle_task: TaskRef) -> Self {
        Self {
            core,
            idle_task,
            queue: imp::Queue::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    pub fn len(&self) -> usize {
        self.queue.len()
    }

    pub fn add(&mut self, task: TaskRef) {
        self.queue.add(task);
        #[cfg(single_simd_task_optimization)]
        if task.simd {
            single_simd_task_optimization::simd_tasks_added_to_core(self.iter(), self.core);
        }
    }

    #[cfg(runqueue_spillful)]
    pub fn remove(&mut self, _: &TaskRef) {
        // For the runqueue state spill evaluation, we disable this method
        // because we only want to allow removing a task from a runqueue
        // from within the TaskRef::internal_exit() method.
    }

    #[cfg(not(runqueue_spillful))]
    pub fn remove(&mut self, task: &TaskRef) {
        self.queue.remove(task);
        #[cfg(single_simd_task_optimization)]
        if task.simd {
            single_simd_task_optimization::simd_tasks_removed_from_core(self.iter(), self.core);
        }
    }

    pub fn next(&mut self) -> TaskRef {
        match self.queue.next() {
            Some(task) => task,
            None => self.idle_task.clone(),
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &TaskRef> {
        self.queue.iter()
    }

    pub fn get_priority(&self, task: &TaskRef) -> Option<u8> {
        self.queue.get_priority(task)
    }

    pub fn set_priority(&mut self, task: &TaskRef, priority: u8) -> Result<(), &'static str> {
        self.queue.set_priority(task, priority)
    }

    pub fn set_periodicity(
        &mut self,
        task: &TaskRef,
        periodicity: usize,
    ) -> Result<(), &'static str> {
        self.queue.set_periodicity(task, periodicity)
    }
}
