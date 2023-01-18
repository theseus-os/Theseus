use crate::TaskRef;
use alloc::collections::VecDeque;
use core::{intrinsics::unlikely, sync::atomic::Ordering};

#[derive(Debug)]
pub(crate) struct Queue {
    inner: VecDeque<RealtimeTaskRef>,
}

#[derive(Debug)]
struct RealtimeTaskRef {
    inner: TaskRef,
    period: Option<usize>,
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
        self.inner.push_back(RealtimeTaskRef {
            inner: task,
            period: None,
        });
    }

    pub(crate) fn remove(&mut self, task: &TaskRef) {
        self.inner.retain(|other_task| other_task.inner != *task);
    }

    pub(crate) fn next(&mut self) -> Option<TaskRef> {
        let mut index = 0;
        // TODO: This is inefficient.
        while index < self.inner.len() {
            let task = &self.inner[index];
            if !task.inner.is_runnable() {
                task.inner.is_on_run_queue.store(false, Ordering::Release);
                // Checking this prevents an interleaving where `TaskRef::unblock` wouldn't add
                // the task back onto the run queue. `TaskRef::unblock` sets the run state and
                // then checks is_on_run_queue so we have to do the opposite.
                //
                // TODO: This could be a relaxed load followed by a fence in the if statement.
                if unlikely(task.inner.is_runnable()) {
                    task.inner.is_on_run_queue.store(true, Ordering::Release);
                    index += 1;
                } else {
                    self.inner.remove(index).unwrap();
                }
            } else {
                index += 1;
            }
        }

        let index = self
            .inner
            .iter()
            .position(|task| task.inner.is_runnable())?;

        let task = self.inner.remove(index).unwrap();
        let task_ref = task.inner.clone();

        self.insert(task);

        Some(task_ref)
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &TaskRef> {
        self.inner.iter().map(|task| &task.inner)
    }

    pub(crate) fn get_priority(&self, _: &TaskRef) -> Option<u8> {
        None
    }

    pub(crate) fn set_priority(&mut self, _: &TaskRef, _: u8) -> Result<(), &'static str> {
        Err("cannot set priority using realtime scheduler")
    }

    pub(crate) fn set_periodicity(
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
