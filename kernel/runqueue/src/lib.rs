//! This crate contains the trait of `RunQueue` structure, Runqueue Structure should contain
//! list of tasks with additional scheduling information depending on the scheduler
//! 

#![no_std]
#![feature(alloc)]

extern crate alloc;
extern crate irq_safety;
extern crate atomic_linked_list;
extern crate task;

#[cfg(single_simd_task_optimization)]
extern crate single_simd_task_optimization;

use irq_safety::{RwLockIrqSafe};
use task::{TaskRef};

/// Runqueue structure holds the list of tasks that are runnable in a given core 
pub trait RunQueueTrait {

    /// Creates a new `RunQueue` for the given core, which is an `apic_id`.
    fn init(which_core: u8) -> Result<(), &'static str>;

    /// Creates a new `RunQueue` for the given core, which is an `apic_id`.
    fn get_runqueue(which_core: u8) -> Option<&'static RwLockIrqSafe<Self>>;

    /// Returns the "least busy" core
    fn get_least_busy_core() -> Option<u8>;

    /// Returns the `RunQueue` for the "least busy" core.
    /// See [`get_least_busy_core()`](#method.get_least_busy_core)
    fn get_least_busy_runqueue() -> Option<&'static RwLockIrqSafe<Self>>;

    /// Chooses the "least busy" core's runqueue
    /// and adds the given `Task` reference to that core's runqueue.
    fn add_task_to_any_runqueue(task: TaskRef) -> Result<(), &'static str>;

    /// Adds the given `Task` reference to given core's runqueue.
    fn add_task_to_specific_runqueue(which_core: u8, task: TaskRef) -> Result<(), &'static str>;

    /// Adds a `TaskRef` to this RunQueue.
    fn add_task(&mut self, task: TaskRef) -> Result<(), &'static str>;

    /// Retrieves the `TaskRef` in this `RunQueue` at the specified `index`.
    /// Index 0 is the front of the RunQueue.
    fn get(&self, index: usize) -> Option<&TaskRef>;

    /// The internal function that actually removes the task from the runqueue.
    fn remove_internal(&mut self, task: &TaskRef) -> Result<(), &'static str>;

    /// Removes a `TaskRef` from this RunQueue.
    fn remove_task(&mut self, task: &TaskRef) -> Result<(), &'static str>;

    #[cfg(runqueue_state_spill_evaluation)]
    /// Removes a `TaskRef` from the RunQueue(s) on the given `core`.
    /// Note: This method is only used by the state spillful runqueue implementation.
    fn remove_task_from_within_task(task: &TaskRef, core: u8) -> Result<(), &'static str>;

    /// Removes a `TaskRef` from all `RunQueue`s that exist on the entire system.
    fn remove_task_from_all(task: &TaskRef) -> Result<(), &'static str>;

}


