//! This crate contains the API of the `RunQueue` structure, Runqueue Structure should contain
//! list of tasks with additional scheduling information depending on the scheduler.
//! All crates except the scheduler should refer to this crate to access functions on `RunQueue`.

#![no_std]

extern crate alloc;

cfg_if::cfg_if! {
    if #[cfg(epoch_scheduler)] {
        extern crate runqueue_epoch as runqueue;
    } else if #[cfg(priority_scheduler)] {
        extern crate runqueue_priority as runqueue;
    } else {
        extern crate runqueue_round_robin as runqueue;
    }
}

use sync_preemption::PreemptionSafeRwLock;
use task_struct::RawTaskRef;
use runqueue::RunQueue;

/// Creates a new `RunQueue` for the given core, which is an `apic_id`.
pub fn init(which_core: u8, idle_task: RawTaskRef) -> Result<(), &'static str> {
    RunQueue::init(which_core, idle_task)
}

/// Returns the `RunQueue` of the given core, which is an `apic_id`.
pub fn get_runqueue(which_core: u8) -> Option<&'static PreemptionSafeRwLock<RunQueue>> {
    RunQueue::get_runqueue(which_core)
}

/// Returns the "least busy" core
pub fn get_least_busy_core() -> Option<u8> {
    RunQueue::get_least_busy_core()
}

/// Chooses the "least busy" core's runqueue
/// and adds the given `Task` reference to that core's runqueue.
pub fn add_task_to_any_runqueue(task: RawTaskRef) -> Result<(), &'static str> {
    RunQueue::add_task_to_any_runqueue(task)
}

/// Adds the given `Task` reference to given core's runqueue.
pub fn add_task_to_specific_runqueue(which_core: u8, task: RawTaskRef) -> Result<(), &'static str> {
    RunQueue::add_task_to_specific_runqueue(which_core, task)
}

/// Removes a `RawTaskRef` from all `RunQueue`s that exist on the entire system.
pub fn remove_task_from_all(task: &RawTaskRef) -> Result<(), &'static str> {
    RunQueue::remove_task_from_all(task)
}
