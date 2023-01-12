use alloc::collections::VecDeque;
use task::TaskRef;

#[derive(Debug)]
pub(crate) struct Queue {
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

    pub(crate) fn add(&mut self, task: TaskRef) {
        self.inner.push_back(task)
    }

    pub(crate) fn remove(&mut self, task: &TaskRef) {
        self.inner.retain(|other_task| other_task != task);
    }

    pub(crate) fn next(&mut self) -> Option<TaskRef> {
        let index = self.inner.iter().position(|task| task.is_runnable())?;

        let task = self.inner.remove(index).unwrap();
        self.inner.push_back(task.clone());
        Some(task)
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &TaskRef> {
        self.inner.iter()
    }

    pub(crate) fn get_priority(&self, _: &TaskRef) -> Option<u8> {
        None
    }

    pub(crate) fn set_priority(&mut self, _: &TaskRef, _: u8) -> Result<(), &'static str> {
        Err("cannot set priority using round robin scheduler")
    }

    pub(crate) fn set_periodicity(&mut self, _: &TaskRef, _: usize) -> Result<(), &'static str> {
        Err("cannot set periodicity using round robin scheduler")
    }
}
