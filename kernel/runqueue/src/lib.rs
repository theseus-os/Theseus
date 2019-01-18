//! This crate contains the trait of `RunQueue` structure, Runqueue Structure should contain
//! list of tasks with additional scheduling information depending on the scheduler
//! 

#![no_std]
#![feature(alloc)]

extern crate alloc;
extern crate irq_safety;
extern crate atomic_linked_list;
extern crate task;
#[cfg(priority_scheduler)] extern crate runqueue_priority;
#[cfg(not(priority_scheduler))] extern crate runqueue_round_robin;

#[cfg(single_simd_task_optimization)]
extern crate single_simd_task_optimization;

use irq_safety::{RwLockIrqSafe};
use task::{TaskRef};
#[cfg(priority_scheduler)] use runqueue_priority::RunQueue;
#[cfg(not(priority_scheduler))] use runqueue_round_robin::RunQueue;


/// Runqueue structure holds the list of tasks that are runnable in a given core 
//pub trait RunQueueTrait {

    /// Creates a new `RunQueue` for the given core, which is an `apic_id`.
    pub fn init(which_core: u8) -> Result<(), &'static str>{
        RunQueue::init(which_core)
    }

    /// Creates a new `RunQueue` for the given core, which is an `apic_id`.
    pub fn get_runqueue(which_core: u8) -> Option<&'static RwLockIrqSafe<RunQueue>>{
        RunQueue::get_runqueue(which_core)
    }

    /// Returns the "least busy" core
    pub fn get_least_busy_core() -> Option<u8>{
        RunQueue::get_least_busy_core()
    }

    /// Returns the `RunQueue` for the "least busy" core.
    /// See [`get_least_busy_core()`](#method.get_least_busy_core)
    pub fn get_least_busy_runqueue() -> Option<&'static RwLockIrqSafe<RunQueue>>{
        RunQueue::get_least_busy_runqueue()
    }

    /// Chooses the "least busy" core's runqueue
    /// and adds the given `Task` reference to that core's runqueue.
    pub fn add_task_to_any_runqueue(task: TaskRef) -> Result<(), &'static str>{
        RunQueue::add_task_to_any_runqueue(task)
    }

    /// Adds the given `Task` reference to given core's runqueue.
    pub fn add_task_to_specific_runqueue(which_core: u8, task: TaskRef) -> Result<(), &'static str>{
        RunQueue::add_task_to_specific_runqueue(which_core, task)
    }

    /// Removes a `TaskRef` from this RunQueue.
    //pub fn remove_task(&mut self, task: &TaskRef) -> Result<(), &'static str>{
    //pub fn remove_task(mut runqueue: &&runqueue_round_robin::RunQueue) -> Result<(), &'static str>{
     //   RunQueue::remove_task(runqueue, task)
    //}

    #[cfg(runqueue_state_spill_evaluation)]
    /// Removes a `TaskRef` from the RunQueue(s) on the given `core`.
    /// Note: This method is only used by the state spillful runqueue implementation.
    pub fn remove_task_from_within_task(task: &TaskRef, core: u8) -> Result<(), &'static str>{
        RunQueue::remove_task_from_within_task(task, core)
    }

    /// Removes a `TaskRef` from all `RunQueue`s that exist on the entire system.
    pub fn remove_task_from_all(task: &TaskRef) -> Result<(), &'static str>{
        RunQueue::remove_task_from_all(task)
    }

//}


