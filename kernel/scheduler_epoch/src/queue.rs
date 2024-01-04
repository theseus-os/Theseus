use alloc::collections::VecDeque;

use bit_set::BitSet;
use task::TaskRef;

use crate::{EpochTaskRef, TaskConfiguration, MAX_PRIORITY};

/// A singular run queue.
///
/// The scheduler contains two of these: an active one, and an expired one.
#[derive(Debug, Clone)]
pub(crate) struct RunQueue {
    // TODO: Encode using MAX_PRIORITY
    priorities: BitSet,
    len: usize,
    inner: [VecDeque<EpochTaskRef>; MAX_PRIORITY as usize],
}

impl RunQueue {
    #[inline]
    pub(crate) const fn new() -> Self {
        const INIT: VecDeque<EpochTaskRef> = VecDeque::new();

        Self {
            priorities: BitSet::new(),
            len: 0,
            inner: [INIT; MAX_PRIORITY as usize],
        }
    }

    #[inline]
    pub(crate) fn len(&self) -> usize {
        debug_assert_eq!(
            self.inner.iter().map(|queue| queue.len()).sum::<usize>(),
            self.len
        );
        self.len
    }

    #[inline]
    pub(crate) fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[inline]
    pub(crate) fn push(&mut self, task: EpochTaskRef, priority: u8) {
        self.priorities.insert(priority);
        self.inner[priority as usize].push_back(task);
        self.len += 1;
    }

    #[inline]
    pub(crate) fn next(&mut self, expired: &mut Self, total_weight: usize) -> Option<TaskRef> {
        let mut priorities = self.priorities.clone();

        let mut top_index = priorities.max()?;
        // TODO: top_queue.len() == 1 optimisation
        let mut top_queue = &mut self.inner[top_index as usize];
        let mut next_task = top_queue.front().unwrap();

        if !next_task.is_runnable() {
            // TODO: This incredibly convoluted code is necessary because we store
            // non-runnable tasks on the run queue.

            // Iterate through the queue to find the next runnable task and bring it to the
            // front of its respective run queue.

            let mut vec_index = 0;

            while !next_task.is_runnable() {
                vec_index += 1;

                if vec_index + 1 == top_queue.len() {
                    priorities.remove(top_index);
                    top_index = match priorities.max() {
                        Some(top) => top,
                        None => {
                            // There are no runnable tasks on the run queue. We
                            // must transfer all the tasks to the expired run
                            // queue and return None.

                            let mut priorities = self.priorities.clone();

                            while let Some(top_index) = priorities.max() {
                                let top_queue = &mut self.inner[top_index as usize];

                                while let Some(mut task) = top_queue.pop_front() {
                                    task.recalculate_tokens(TaskConfiguration {
                                        priority: top_index as usize,
                                        total_weight,
                                    });
                                    expired.push(task, top_index);
                                }

                                priorities.remove(top_index);
                            }

                            return None;
                        }
                    };
                    vec_index = 0;
                }

                top_queue = &mut self.inner[top_index as usize];
                next_task = &top_queue[vec_index];
            }

            for _ in 0..vec_index {
                let task = top_queue.pop_front().unwrap();
                top_queue.push_back(task);
            }
        }

        let queue = &mut self.inner[top_index as usize];
        let next_task = queue.front().unwrap();

        Some(if next_task.tokens <= 1 {
            let mut next_task = queue.pop_front().unwrap();
            self.len -= 1;

            next_task.recalculate_tokens(TaskConfiguration {
                priority: top_index as usize,
                total_weight,
            });
            expired.push(next_task.clone(), top_index);

            if queue.is_empty() {
                self.priorities.remove(top_index);
            }

            next_task.clone().task
        } else {
            let mut next_task = queue.pop_front().unwrap();

            next_task.tokens -= 1;
            queue.push_back(next_task.clone());

            next_task.task
        })
    }

    #[inline]
    fn top_index(&self) -> Option<usize> {
        self.priorities.max().map(|priority| priority as usize)
    }

    #[inline]
    pub(crate) fn remove(&mut self, task: &TaskRef) -> bool {
        for i in self.priorities.iter() {
            let queue = &mut self.inner[i];

            for j in 0..queue.len() {
                let element = &queue[j];

                if **element == *task {
                    queue.remove(j);
                    self.len -= 1;

                    if queue.is_empty() {
                        self.priorities.remove(i as u8);
                    }

                    return true;
                }
            }
        }
        false
    }

    /// Returns the priority of the given task.
    #[inline]
    pub(crate) fn priority(&self, task: &TaskRef) -> Option<u8> {
        for i in self.priorities.iter() {
            let queue = &self.inner[i];
            for t in queue {
                if **t == *task {
                    return Some(i as u8);
                }
            }
        }
        None
    }

    /// Sets the priority of the given task.
    ///
    /// Returns `true` if an action was performed i.e. if the task was in the
    /// run queue.
    #[inline]
    pub(crate) fn set_priority(&mut self, task: &TaskRef, priority: u8) -> bool {
        for i in self.priorities.iter() {
            let queue = &mut self.inner[i];

            for j in 0..queue.len() {
                let element = &queue[j];

                if **element == *task {
                    let task = queue.remove(j).unwrap();
                    self.len -= 1;

                    if queue.is_empty() {
                        self.priorities.remove(i as u8);
                    }

                    self.push(task, priority);
                    return true;
                }
            }
        }
        false
    }

    #[inline]
    pub(crate) fn drain(self) -> Drain {
        Drain { inner: self }
    }
}

impl IntoIterator for RunQueue {
    type Item = TaskRef;

    type IntoIter = Drain;

    fn into_iter(self) -> Self::IntoIter {
        self.drain()
    }
}

pub(crate) struct Drain {
    inner: RunQueue,
}

impl Iterator for Drain {
    type Item = TaskRef;

    fn next(&mut self) -> Option<Self::Item> {
        let top_index = self.inner.top_index()?;
        let top_queue = &mut self.inner.inner[top_index];

        if top_queue.len() == 1 {
            self.inner.priorities.remove(top_index as u8);
        }

        Some(top_queue.pop_front().unwrap().into())
    }
}
