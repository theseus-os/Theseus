use alloc::collections::VecDeque;
use core::cmp::{max, min};
use task::TaskRef;

const MAX_PRIORITY: u8 = 40;
const DEFAULT_PRIORITY: u8 = 20;
const INITIAL_TOKENS: usize = 10;

#[derive(Debug)]
pub(crate) struct Queue {
    inner: VecDeque<PriorityTaskRef>,
}

#[derive(Debug)]
struct PriorityTaskRef {
    inner: TaskRef,
    priority: u8,
    tokens: usize,
}

impl Queue {
    pub(crate) fn new() -> Self {
        Self {
            inner: VecDeque::new(),
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    pub(crate) fn len(&self) -> usize {
        self.inner.len()
    }

    pub(crate) fn add(&mut self, task: TaskRef) {
        self.inner.push_back(PriorityTaskRef {
            inner: task,
            priority: DEFAULT_PRIORITY,
            tokens: INITIAL_TOKENS,
        });
    }

    pub(crate) fn remove(&mut self, task: &TaskRef) {
        self.inner.retain(|other_task| other_task.inner != *task);
    }

    pub(crate) fn next(&mut self) -> Option<TaskRef> {
        if let Some(task) = self.try_next() {
            Some(task)
        } else {
            self.distribute_tokens();
            self.try_next()
        }
    }

    fn try_next(&mut self) -> Option<TaskRef> {
        let index = self
            .inner
            .iter()
            .position(|task| task.inner.is_runnable() && task.tokens > 0)?;
        let mut task = self.inner.remove(index).unwrap();
        task.tokens = task.tokens.checked_sub(1).unwrap();

        let task_ref = task.inner.clone();

        self.inner.push_back(task);
        Some(task_ref)
    }

    pub(crate) fn distribute_tokens(&mut self) {
        let mut total_priorities = 1;
        for task in self.inner.iter().filter(|task| task.inner.is_runnable()) {
            total_priorities += 1 + task.priority as usize;
        }

        let epoch = max(total_priorities, 100);

        for task in self
            .inner
            .iter_mut()
            .filter(|task| task.inner.is_runnable())
        {
            task.tokens = epoch
                .saturating_mul((task.priority as usize).saturating_add(1))
                .wrapping_div(total_priorities);
        }
    }

    pub(crate) fn get_priority(&self, task: &TaskRef) -> Option<u8> {
        self.inner
            .iter()
            .find(|other_task| other_task.inner == *task)
            .map(|task| task.priority)
    }

    pub(crate) fn set_priority(
        &mut self,
        task: &TaskRef,
        priority: u8,
    ) -> Result<(), &'static str> {
        if let Some(task) = self
            .inner
            .iter_mut()
            .find(|other_task| other_task.inner == *task)
        {
            task.priority = min(priority, MAX_PRIORITY);
        }
        Ok(())
    }

    pub(crate) fn set_periodicity(&mut self, _: &TaskRef, _: usize) -> Result<(), &'static str> {
        Err("cannot set periodicity using priority scheduler")
    }
}
