use alloc::collections::VecDeque;
use task::TaskRef;

#[derive(Debug)]
pub struct Queue {
    inner: VecDeque<TaskRef>,
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

    pub(crate) fn add(&mut self, task: task::TaskRef) {
        self.inner.push_back(task)
    }

    pub(crate) fn remove(&mut self, task: &task::TaskRef) {
        self.inner.retain(|t| t != task);
    }

    pub(crate) fn next(&mut self) -> Option<task::TaskRef> {
        for (i, task) in self.inner.iter().enumerate() {
            // we skip the idle task, and only choose it if no other tasks are runnable
            if task.is_an_idle_task {
                panic!();
            }

            // must be runnable
            if !task.is_runnable() {
                continue;
            }

            let task = self.inner.swap_remove_front(i).unwrap();
            self.inner.push_back(task.clone());
            return Some(task);
        }

        return None;
    }
}

pub fn set_priority(_: &TaskRef, _: u8) -> Result<(), &'static str> {
    Err("cannot set priority using round robin scheduler")
}

pub fn get_priority(_: &TaskRef) -> Option<usize> {
    None
}

pub fn set_periodicity(_: &TaskRef, _: usize) -> Result<(), &'static str> {
    Err("cannot set periodicity using round robin scheduler")
}
