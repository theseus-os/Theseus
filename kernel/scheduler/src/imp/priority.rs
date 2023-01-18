use crate::TaskRef;
use alloc::{collections::VecDeque, vec::Vec};
use core::{
    cmp::{max, min},
    intrinsics::unlikely,
    sync::atomic::Ordering,
};

const MAX_PRIORITY: u8 = 40;
const DEFAULT_PRIORITY: u8 = 20;
const INITIAL_TOKENS: usize = 10;

#[derive(Debug)]
pub(crate) struct Queue {
    have_tokens: VecDeque<PriorityTaskRef>,
    out_of_tokens: Vec<PriorityTaskRef>,
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
            have_tokens: VecDeque::new(),
            out_of_tokens: Vec::new(),
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.have_tokens.is_empty() && self.out_of_tokens.is_empty()
    }

    pub(crate) fn len(&self) -> usize {
        self.have_tokens.len() + self.out_of_tokens.len()
    }

    pub(crate) fn add(&mut self, task: TaskRef) {
        self.have_tokens.push_back(PriorityTaskRef {
            inner: task,
            priority: DEFAULT_PRIORITY,
            tokens: INITIAL_TOKENS,
        });
    }

    pub(crate) fn remove(&mut self, task: &TaskRef) {
        self.have_tokens
            .retain(|other_task| other_task.inner != *task);
        self.out_of_tokens
            .retain(|other_task| other_task.inner != *task);
    }

    pub(crate) fn next(&mut self) -> Option<TaskRef> {
        if let Some(task) = self.try_next() {
            Some(task)
        } else {
            self.distribute_tokens();
            self.try_next()
        }
    }

    fn distribute_tokens(&mut self) {
        while let Some(task) = self.out_of_tokens.pop() {
            self.have_tokens.push_back(task);
        }

        let mut total_priorities = 1;
        for task in self
            .have_tokens
            .iter()
            .filter(|task| task.inner.is_runnable())
        {
            total_priorities += 1 + task.priority as usize;
        }

        let epoch = max(total_priorities, 100);

        for task in self
            .have_tokens
            .iter_mut()
            .filter(|task| task.inner.is_runnable())
        {
            task.tokens = epoch
                .saturating_mul((task.priority as usize).saturating_add(1))
                .wrapping_div(total_priorities);
        }
    }

    fn try_next(&mut self) -> Option<TaskRef> {
        while let Some(task) = self.have_tokens.pop_front() {
            if task.inner.is_runnable() {
                if let Some(task) = self.add_priority_task(task) {
                    return Some(task);
                }
            } else {
                task.inner.is_on_run_queue.store(false, Ordering::Release);
                // Checking this prevents an interleaving where `TaskRef::unblock` wouldn't add
                // the task back onto the run queue. `TaskRef::unblock` sets the run state and
                // then checks is_on_run_queue so we have to do the opposite.
                //
                // TODO: This could be a relaxed load followed by a fence in the if statement.
                if unlikely(task.inner.is_runnable()) {
                    task.inner.is_on_run_queue.store(true, Ordering::Release);
                    if let Some(task) = self.add_priority_task(task) {
                        return Some(task);
                    }
                }
            }
        }
        None
    }

    fn add_priority_task(&mut self, mut task: PriorityTaskRef) -> Option<TaskRef> {
        match task.tokens.checked_sub(1) {
            Some(new_tokens) => {
                task.tokens = new_tokens;
                let task_ref = task.inner.clone();
                self.have_tokens.push_back(task);
                Some(task_ref)
            }
            None => {
                self.out_of_tokens.push(task);
                None
            }
        }
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &TaskRef> {
        self.have_tokens
            .iter()
            .map(|task| &task.inner)
            .chain(self.out_of_tokens.iter().map(|task| &task.inner))
    }

    pub(crate) fn get_priority(&self, task: &TaskRef) -> Option<u8> {
        self.have_tokens
            .iter()
            .chain(self.out_of_tokens.iter())
            .find(|other_task| other_task.inner == *task)
            .map(|task| task.priority)
    }

    pub(crate) fn set_priority(
        &mut self,
        task: &TaskRef,
        priority: u8,
    ) -> Result<(), &'static str> {
        if let Some(task) = self
            .have_tokens
            .iter_mut()
            .chain(self.out_of_tokens.iter_mut())
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
