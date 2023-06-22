//! This crate contains the `RunQueue` structure, for the epoch scheduler.
//! `RunQueue` structure is essentially a list of Tasks
//! that it used for scheduling purposes.

#![no_std]
#![feature(let_chains)]

extern crate alloc;

use alloc::collections::VecDeque;
use atomic_linked_list::atomic_map::AtomicMap;
use core::ops::{Deref, DerefMut};
use log::{debug, error, trace};
use sync_preemption::PreemptionSafeRwLock;
use task::TaskRef;

pub const MAX_PRIORITY: u8 = 40;
pub const DEFAULT_PRIORITY: u8 = 20;
pub const INITIAL_TOKENS: usize = 10;

#[derive(Debug, Clone)]
pub struct EpochTaskRef {
    task: TaskRef,
    pub priority: u8,
    /// Remaining tokens in this epoch. A task will be scheduled in an epoch
    /// until tokens run out
    pub tokens_remaining: usize,
}

impl Deref for EpochTaskRef {
    type Target = TaskRef;

    fn deref(&self) -> &TaskRef {
        &self.task
    }
}

impl DerefMut for EpochTaskRef {
    fn deref_mut(&mut self) -> &mut TaskRef {
        &mut self.task
    }
}

impl EpochTaskRef {
    /// Creates a new `EpochTaskRef` that wraps the given `TaskRef`.
    /// We just give an initial number of tokens to run the task till
    /// next scheduling epoch
    pub fn new(task: TaskRef) -> EpochTaskRef {
        EpochTaskRef {
            task,
            priority: DEFAULT_PRIORITY,
            tokens_remaining: INITIAL_TOKENS,
        }
    }
}

/// There is one runqueue per core, each core only accesses its own private
/// runqueue and allows the scheduler to select a task from that runqueue to
/// schedule in.
static RUNQUEUES: AtomicMap<u8, PreemptionSafeRwLock<RunQueue>> = AtomicMap::new();

/// A list of references to `Task`s (`EpochTaskRef`s)
/// that is used to store the `Task`s (and associated scheduler related data)
/// that are runnable on a given core.
/// A queue is used for the token based epoch scheduler.
/// `Runqueue` implements `Deref` and `DerefMut` traits, which dereferences to
/// `VecDeque`.
#[derive(Debug)]
pub struct RunQueue {
    core: u8,
    queue: VecDeque<EpochTaskRef>,
    idle_task: TaskRef,
}

impl Deref for RunQueue {
    type Target = VecDeque<EpochTaskRef>;

    fn deref(&self) -> &Self::Target {
        &self.queue
    }
}

impl DerefMut for RunQueue {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.queue
    }
}

impl RunQueue {
    /// Moves the `TaskRef` at the given index in this `RunQueue` to the end
    /// (back) of this `RunQueue`, and returns a cloned reference to that
    /// `TaskRef`. The number of tokens is reduced by one and number of context
    /// switches is increased by one. This function is used when the task is
    /// selected by the scheduler
    pub fn update_and_move_to_end(&mut self, index: usize, tokens: usize) -> Option<TaskRef> {
        if let Some(mut priority_task_ref) = self.remove(index) {
            priority_task_ref.tokens_remaining = tokens;
            let task_ref = priority_task_ref.task.clone();
            self.push_back(priority_task_ref);
            Some(task_ref)
        } else {
            None
        }
    }

    /// Creates a new `RunQueue` for the given core, which is an `apic_id`
    pub fn init(which_core: u8, idle_task: TaskRef) -> Result<(), &'static str> {
        #[cfg(not(loscd_eval))]
        trace!("Created runqueue (priority) for core {}", which_core);
        let new_rq = PreemptionSafeRwLock::new(RunQueue {
            core: which_core,
            queue: VecDeque::new(),
            idle_task,
        });

        if RUNQUEUES.insert(which_core, new_rq).is_some() {
            error!(
                "BUG: RunQueue::init(): runqueue already exists for core {}!",
                which_core
            );
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

        rq.write().add_task(task)
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
            .add_task(task)
    }

    /// Adds a `TaskRef` to this RunQueue.
    fn add_task(&mut self, task: TaskRef) -> Result<(), &'static str> {
        #[cfg(not(loscd_eval))]
        debug!("Adding task to runqueue_priority {}, {:?}", self.core, task);
        let priority_task_ref = EpochTaskRef::new(task);
        self.push_back(priority_task_ref);

        #[cfg(single_simd_task_optimization)]
        {
            warn!("USING SINGLE_SIMD_TASK_OPTIMIZATION VERSION OF RUNQUEUE::ADD_TASK");
            // notify simd_personality crate about runqueue change, but only for SIMD tasks
            if task.simd {
                single_simd_task_optimization::simd_tasks_added_to_core(self.iter(), self.core);
            }
        }

        Ok(())
    }

    /// Removes a `TaskRef` from this RunQueue.
    pub fn remove_task(&mut self, task: &TaskRef) -> Result<(), &'static str> {
        debug!(
            "Removing task from runqueue_priority {}, {:?}",
            self.core, task
        );
        self.retain(|x| &x.task != task);

        #[cfg(single_simd_task_optimization)]
        {
            warn!("USING SINGLE_SIMD_TASK_OPTIMIZATION VERSION OF RUNQUEUE::REMOVE_TASK");
            // notify simd_personality crate about runqueue change, but only for SIMD tasks
            if task.simd {
                single_simd_task_optimization::simd_tasks_removed_from_core(self.iter(), self.core);
            }
        }

        Ok(())
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

    pub fn idle_task(&self) -> &TaskRef {
        &self.idle_task
    }

    fn get_priority(&self, task: &TaskRef) -> Option<u8> {
        for epoch_task in self.iter() {
            if &epoch_task.task == task {
                return Some(epoch_task.priority);
            }
        }
        None
    }

    /// Sets the priority of the given task.
    ///
    /// Returns whether the task was found in the run queue.
    fn set_priority(&mut self, task: &TaskRef, priority: u8) -> bool {
        for epoch_task in self.iter_mut() {
            if &epoch_task.task == task {
                epoch_task.priority = priority;
                return true;
            }
        }
        false
    }
}

/// Returns the priority of the given task if it exists, otherwise none.
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

/// Modifies the given task's priority to be the maximum of its priority and the
/// current task's priority.
pub fn inherit_priority(task: &TaskRef) -> impl FnOnce() + '_ {
    let current_task = task::get_my_current_task().unwrap();

    let mut current_priority = None;
    let mut other_priority = None;

    'outer: for (core, run_queue) in RUNQUEUES.iter() {
        for epoch_task in run_queue.read().iter() {
            if epoch_task.task == current_task {
                current_priority = Some(epoch_task.priority);
                if other_priority.is_some() {
                    break 'outer;
                }
            } else if &epoch_task.task == task {
                other_priority = Some((core, epoch_task.priority));
                if current_priority.is_some() {
                    break 'outer;
                }
            }
        }
    }

    if let (Some(current_priority), Some((core, other_priority))) =
        (current_priority, other_priority) && current_priority > other_priority
    {
        // NOTE: This assumes no task migration.
        debug_assert!(RUNQUEUES.get(core).unwrap().write().set_priority(task, current_priority));
    }

    move || {
        if let (Some(current_priority), Some((_, other_priority))) =
            (current_priority, other_priority) && current_priority > other_priority
        {
            set_priority(task, other_priority);
        }
    }
}
