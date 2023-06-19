//! Runqueue structures for a priority scheduler.
//!
//! The `RunQueue` structure is essentially a list of `Task`s used for
//! scheduling purposes. Each `PriorityTaskRef` element in the runqueue contains
//! a `TaskRef` representing an underlying task and as well as a `period` value.

#![no_std]
// TODO: Update Rust version to stabilise.
#![feature(binary_heap_retain)]

extern crate alloc;

use alloc::collections::BinaryHeap;
use atomic_linked_list::atomic_map::AtomicMap;
use core::ops::{Deref, DerefMut};
use log::{error, trace};
use sync_preemption::PreemptionSafeRwLock;
use task::TaskRef;

const DEFAULT_PRIORITY: u8 = 0;

/// A reference to a task with its period for priority scheduling.
///
/// `PriorityTaskRef` implements `Deref` and `DerefMut` traits, which
/// dereferences to `TaskRef`.
#[derive(Debug, Clone)]
pub struct PriorityTaskRef {
    task: TaskRef,
    priority: u8,
}

impl PartialEq for PriorityTaskRef {
    fn eq(&self, other: &Self) -> bool {
        self.priority.eq(&other.priority)
    }
}

// The equivalence relation is reflexive.
impl Eq for PriorityTaskRef {}

impl PartialOrd for PriorityTaskRef {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PriorityTaskRef {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.priority.cmp(&other.priority)
    }
}

impl Deref for PriorityTaskRef {
    type Target = TaskRef;

    fn deref(&self) -> &TaskRef {
        &self.task
    }
}

impl DerefMut for PriorityTaskRef {
    fn deref_mut(&mut self) -> &mut TaskRef {
        &mut self.task
    }
}

/// There is one runqueue per core, each core only accesses its own private
/// runqueue and allows the scheduler to select a task from that runqueue to
/// schedule in
static RUNQUEUES: AtomicMap<u8, PreemptionSafeRwLock<RunQueue>> = AtomicMap::new();

/// A list of `Task`s and their associated priority scheduler data that may be
/// run on a given CPU core.
///
/// In rate monotonic scheduling, tasks are sorted in order of increasing
/// periods. Thus, the `period` value acts as a form of task "priority",
/// with higher priority (shorter period) tasks coming first.
#[derive(Debug)]
pub struct RunQueue {
    core: u8,
    queue: BinaryHeap<PriorityTaskRef>,
}

impl Deref for RunQueue {
    type Target = BinaryHeap<PriorityTaskRef>;

    fn deref(&self) -> &BinaryHeap<PriorityTaskRef> {
        &self.queue
    }
}

impl DerefMut for RunQueue {
    fn deref_mut(&mut self) -> &mut BinaryHeap<PriorityTaskRef> {
        &mut self.queue
    }
}

impl RunQueue {
    pub fn init(which_core: u8) -> Result<(), &'static str> {
        #[cfg(not(loscd_eval))]
        trace!("Created runqueue (priority) for core {}", which_core);

        let new_rq = PreemptionSafeRwLock::new(RunQueue {
            core: which_core,
            queue: BinaryHeap::new(),
        });

        if RUNQUEUES.insert(which_core, new_rq).is_some() {
            error!("BUG: RunQueue::init(): runqueue already exists for core {which_core}!");
            Err("runqueue already exists for this core")
        } else {
            // there shouldn't already be a RunQueue for this core
            Ok(())
        }
    }

    /// Returns `RunQueue` for the given core, which is an `apic_id`.
    pub fn get_runqueue(which_core: u8) -> Option<&'static PreemptionSafeRwLock<RunQueue>> {
        RUNQUEUES.get(&which_core)
    }

    /// Returns the "least busy" core, which is currently very simple, based on
    /// runqueue size.
    pub fn get_least_busy_core() -> Option<u8> {
        Self::get_least_busy_runqueue().map(|rq| rq.read().core)
    }

    /// Returns the `RunQueue` for the "least busy" core.
    /// See [`get_least_busy_core()`](#method.get_least_busy_core)
    fn get_least_busy_runqueue() -> Option<&'static PreemptionSafeRwLock<RunQueue>> {
        let mut min_rq: Option<(&'static PreemptionSafeRwLock<RunQueue>, usize)> = None;

        for (_, rq) in RUNQUEUES.iter() {
            let rq_size = rq.read().queue.len();

            if let Some(min) = min_rq {
                if rq_size < min.1 {
                    min_rq = Some((rq, rq_size));
                }
            } else {
                min_rq = Some((rq, rq_size));
            }
        }

        min_rq.map(|m| m.0)
    }

    /// Chooses the "least busy" core's runqueue (based on simple
    /// runqueue-size-based load balancing) and adds the given `Task`
    /// reference to that core's runqueue.
    pub fn add_task_to_any_runqueue(task: TaskRef) -> Result<(), &'static str> {
        let rq = RunQueue::get_least_busy_runqueue()
            .or_else(|| RUNQUEUES.iter().next().map(|r| r.1))
            .ok_or("couldn't find any runqueues to add the task to!")?;

        rq.write().add_task(task, DEFAULT_PRIORITY)
    }

    /// Convenience method that adds the given `Task` reference to given core's
    /// runqueue.
    pub fn add_task_to_specific_runqueue(
        which_core: u8,
        task: TaskRef,
    ) -> Result<(), &'static str> {
        RunQueue::get_runqueue(which_core)
            .ok_or("Couldn't get RunQueue for the given core")?
            .write()
            .add_task(task, DEFAULT_PRIORITY)
    }

    /// Adds a `TaskRef` to this runqueue with the given periodicity value
    fn add_task(&mut self, task: TaskRef, priority: u8) -> Result<(), &'static str> {
        let priority_task = PriorityTaskRef { task, priority };
        self.queue.push(priority_task);
        Ok(())
    }

    /// The internal function that actually removes the task from the runqueue.
    fn remove_internal(&mut self, task: &TaskRef) -> Result<(), &'static str> {
        self.queue.retain(|x| &x.task != task);
        Ok(())
    }

    /// Removes a `TaskRef` from this RunQueue.
    pub fn remove_task(&mut self, task: &TaskRef) -> Result<(), &'static str> {
        self.remove_internal(task)
    }

    /// Removes a `TaskRef` from all `RunQueue`s that exist on the entire
    /// system.
    ///
    /// This is a brute force approach that iterates over all runqueues.
    pub fn remove_task_from_all(task: &TaskRef) -> Result<(), &'static str> {
        for (_core, rq) in RUNQUEUES.iter() {
            rq.write().remove_task(task)?;
        }
        Ok(())
    }

    fn get_priority(&self, task: &TaskRef) -> Option<u8> {
        for t in self.queue.iter() {
            if t.task == *task {
                return Some(t.priority);
            }
        }
        None
    }

    fn set_priority(&mut self, task: &TaskRef, priority: u8) -> bool {
        let previous_len = self.queue.len();
        self.queue.retain(|t| t.task != *task);

        if previous_len != self.queue.len() {
            debug_assert_eq!(self.queue.len() + 1, previous_len);
            self.queue.push(PriorityTaskRef {
                // TODO: Don't take reference?
                task: task.clone(),
                priority,
            });
            true
        } else {
            false
        }
    }
}

pub fn get_priority(task: &TaskRef) -> Option<u8> {
    for (_, run_queue) in RUNQUEUES.iter() {
        if let Some(priority) = run_queue.read().get_priority(task) {
            return Some(priority);
        }
    }
    None
}

pub fn set_priority(task: &TaskRef, priority: u8) {
    for (_, run_queue) in RUNQUEUES.iter() {
        if run_queue.write().set_priority(task, priority) {
            break;
        }
    }
}
