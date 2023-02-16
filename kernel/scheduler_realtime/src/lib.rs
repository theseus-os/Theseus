#![no_std]

extern crate alloc;

use alloc::collections::VecDeque;
use task::TaskRef;

#[derive(Debug)]
pub struct Queue {
    inner: VecDeque<RealtimeTaskRef>,
}

#[derive(Debug)]
struct RealtimeTaskRef {
    inner: TaskRef,
    period: Option<usize>,
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
        self.inner.push_back(RealtimeTaskRef {
            inner: task,
            period: None,
        });
    }

    pub fn remove(&mut self, task: &TaskRef) {
        self.inner.retain(|other_task| other_task.inner != *task);
    }

    pub fn next(&mut self) -> Option<TaskRef> {
        let index = self
            .inner
            .iter()
            .position(|task| task.inner.is_runnable())?;

        let task = self.inner.remove(index).unwrap();
        let task_ref = task.inner.clone();

        self.insert(task);

        Some(task_ref)
    }

    pub fn iter(&self) -> impl Iterator<Item = &TaskRef> {
        self.inner.iter().map(|task| &task.inner)
    }

    pub fn get_priority(&self, _: &TaskRef) -> Option<u8> {
        None
    }

    pub fn set_priority(&mut self, _: &TaskRef, _: u8) -> Result<(), &'static str> {
        Err("cannot set priority using realtime scheduler")
    }

    pub fn set_periodicity(
        &mut self,
        task: &TaskRef,
        periodicity: usize,
    ) -> Result<(), &'static str> {
        if let Some(index) = self
            .inner
            .iter()
            .position(|other_task| other_task.inner == *task)
        {
            let mut task = self.inner.remove(index).unwrap();
            task.period = Some(periodicity);
            self.insert(task);
        }

        Ok(())
    }

    fn insert(&mut self, task: RealtimeTaskRef) {
        match task.period {
            Some(period) if !self.is_empty() => {
                let mut index = 0;

                for other_task in self.inner.iter() {
                    match other_task.period {
                        Some(other_period) if period < other_period => {
                            break;
                        }
                        _ => index += 1,
                    }
                }

                self.inner.insert(index, task);
            }
            _ => self.inner.push_back(task),
        }
    }
}
