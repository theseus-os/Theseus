//! This crate contains the `RunQueue` structure, for round robin scheduler. 
//! `RunQueue` structure is essentially a list of Tasks
//! that is used for scheduling purposes.
//! 

#![no_std]

extern crate alloc;

use alloc::{collections::VecDeque, vec::Vec};
use core::ops::{Deref, DerefMut};
use runqueue_trait::{RunqueueError, RunqueueId, RunqueueTrait, ErasedGenericSchedulerPolicy, GenericSchedulerPolicy};
use task::TaskRef;

/// A cloneable reference to a `Taskref` that exposes more methods
/// related to task scheduling
/// 
/// The `RoundRobinTaskRef` type is necessary since differnt scheduling algorithms 
/// require different data associated with the task to be stored alongside.
/// This makes storing them alongside the task prohibitive.
/// Since round robin is the most primitive scheduling policy 
/// no additional scheduling information is needed.
/// context_switches indicate the number of context switches
/// the task has undergone.
/// context_switches is not used in scheduling algorithm.
/// `RoundRobinTaskRef` implements `Deref` and `DerefMut` traits, which dereferences to `TaskRef`.  
#[derive(Debug, Clone)]
pub struct RoundRobinTaskRef{
    /// `TaskRef` wrapped by `RoundRobinTaskRef`
    taskref: TaskRef,

    /// Number of context switches the task has undergone. Not used in scheduling algorithm
    context_switches: usize,
}

// impl Drop for RoundRobinTaskRef {
//     fn drop(&mut self) {
//         warn!("DROPPING RoundRobinTaskRef with taskref {:?}", self.taskref);
//     }
// }

impl Deref for RoundRobinTaskRef {
    type Target = TaskRef;
    fn deref(&self) -> &TaskRef {
        &self.taskref
    }
}
impl DerefMut for RoundRobinTaskRef {
    fn deref_mut(&mut self) -> &mut TaskRef {
        &mut self.taskref
    }
}
impl From<TaskRef> for RoundRobinTaskRef {
    fn from(taskref: TaskRef) -> Self {
        Self { taskref, context_switches: 0 }
    }
}

impl RoundRobinTaskRef {
    /// Creates a new `RoundRobinTaskRef` that wraps the given `TaskRef`.
    pub fn new(taskref: TaskRef) -> RoundRobinTaskRef {
        RoundRobinTaskRef {
            taskref,
            context_switches: 0,
        }
    }

    /// Increment the number of times the task is picked
    pub fn increment_context_switches(&mut self) {
        self.context_switches = self.context_switches.saturating_add(1);
    }
}


/// A list of references to `Task`s (`RoundRobinTaskRef`s). 
/// This is used to store the `Task`s (and associated scheduler related data) 
/// that are runnable on a given core.
/// A queue is used for the round robin scheduler.
/// `Runqueue` implements `Deref` and `DerefMut` traits, which dereferences to `VecDeque`.
#[derive(Debug)]
pub struct RunqueueRoundRobin {
    id: RunqueueId,
    queue: VecDeque<RoundRobinTaskRef>,
}
// impl Drop for RunQueue {
//     fn drop(&mut self) {
//         warn!("DROPPING Round Robing Runqueue for core {}", self.core);
//     }
// }

impl RunqueueTrait for RunqueueRoundRobin {
    fn id(&self) -> runqueue_trait::RunqueueId {
        self.id
    }
    fn len(&self) -> usize {
        self.queue.len()
    }
    fn task_iter(&self) -> runqueue_trait::TaskIter {
        runqueue_trait::TaskIter::from(
            self.queue.iter().map(Deref::deref).cloned().collect::<Vec<_>>()
        )
    }
}

impl Deref for RunqueueRoundRobin {
    type Target = VecDeque<RoundRobinTaskRef>;
    fn deref(&self) -> &VecDeque<RoundRobinTaskRef> {
        &self.queue
    }
}

impl DerefMut for RunqueueRoundRobin {
    fn deref_mut(&mut self) -> &mut VecDeque<RoundRobinTaskRef> {
        &mut self.queue
    }
}

impl RunqueueRoundRobin {
    pub const fn new(id: RunqueueId) -> Self {
        Self {
            id,
            queue: VecDeque::new(),
        }

    }

    /// Moves the `TaskRef` at the given index into this `RunQueue` to the end (back) of this `RunQueue`,
    /// and returns a cloned reference to that `TaskRef`.
    pub fn move_to_end(&mut self, index: usize) -> Option<TaskRef> {
        self.swap_remove_front(index).map(|rr_taskref| {
            let taskref = rr_taskref.taskref.clone();
            self.push_back(rr_taskref);
            taskref
        })
    }

    /// Adds a `TaskRef` to this RunQueue.
    pub fn add_task(&mut self, taskref: impl Into<RoundRobinTaskRef>) -> Result<(), RunqueueError> {
        let rr_taskref = taskref.into();
        #[cfg(not(rq_eval))]
        log::debug!("Adding task to runqueue_round_robin {:?}, {:?}", self.id, rr_taskref.taskref);

        self.push_back(rr_taskref);
        
        #[cfg(single_simd_task_optimization)]
        {   
            warn!("USING SINGLE_SIMD_TASK_OPTIMIZATION VERSION OF RUNQUEUE::ADD_TASK");
            // notify simd_personality crate about runqueue change, but only for SIMD tasks
            if rr_taskref.task.simd {
                single_simd_task_optimization::simd_tasks_added_to_core(self.iter(), self.id);
            }
        }

        Ok(())
    }

    /// Removes a `TaskRef` from this RunQueue.
    pub fn remove_task(&mut self, task: &TaskRef) -> Result<(), RunqueueError> {
        #[cfg(not(rq_eval))]
        log::debug!("Removing task from runqueue_round_robin {:?}, {:?}", self.id, task);
        self.retain(|x| &x.taskref != task);

        #[cfg(single_simd_task_optimization)] {   
            warn!("USING SINGLE_SIMD_TASK_OPTIMIZATION VERSION OF RUNQUEUE::REMOVE_TASK");
            // notify simd_personality crate about runqueue change, but only for SIMD tasks
            if task.simd {
                single_simd_task_optimization::simd_tasks_removed_from_core(self.iter(), self.id);
            }
        }

        Ok(())
    }
}



///////////////////////////////////////////////////////////////////////////////////////////
//////////////////  Attempt at an erased object-safe trait ////////////////////////////////
///////////////////////////////////////////////////////////////////////////////////////////

impl runqueue_trait::SimpleRunqueueTrait for RunqueueRoundRobin {
    fn id(&self) -> RunqueueId {
        self.id
    }
    fn len(&self) -> usize {
        self.queue.len()
    }
}
