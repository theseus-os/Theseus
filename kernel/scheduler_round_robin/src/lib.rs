#![no_std]

extern crate alloc;

use alloc::collections::VecDeque;
use task::TaskRef;

#[derive(Debug)]
pub struct Queue {
    inner: VecDeque<TaskRef>,
}

impl Queue {
    pub fn new() -> Self {
        Self {
            inner: VecDeque::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn add(&mut self, task: TaskRef) {
        self.inner.push_back(task)
    }

    pub fn remove(&mut self, task: &TaskRef) {
        self.inner.retain(|other_task| other_task != task);
    }

    pub fn next(&mut self) -> Option<TaskRef> {
        let index = self.inner.iter().position(|task| task.is_runnable())?;

        let task = self.inner.remove(index).unwrap();
        self.inner.push_back(task.clone());
        Some(task)
    }

    pub fn iter(&self) -> impl Iterator<Item = &TaskRef> {
        self.inner.iter()
    }

    pub fn get_priority(&self, _: &TaskRef) -> Option<u8> {
        None
    }

    pub fn set_priority(&mut self, _: &TaskRef, _: u8) -> Result<(), &'static str> {
        Err("cannot set priority using round robin scheduler")
    }

    pub fn set_periodicity(&mut self, _: &TaskRef, _: usize) -> Result<(), &'static str> {
        Err("cannot set periodicity using round robin scheduler")
    }
}
