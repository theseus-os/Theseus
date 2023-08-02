//! Offers the ability to control or configure the active task scheduling
//! policy.
//!
//! ## What is and isn't in this crate?
//! This crate also defines the timer interrupt handler used for preemptive
//! task switching on each CPU. In [`init()`], it registers that handler
//! with the [`interrupts`] subsystem.
//!
//! The actual task switching logic is implemented in the [`task`] crate.
//! This crate re-exports that main [`schedule()`] function for convenience,
//! legacy compatbility, and to act as an easy landing page for code search.
//! That means that a caller need only depend on [`task`], not this crate,
//! to invoke the scheduler (yield the CPU) to switch to another task.

#![no_std]
#![cfg_attr(target_arch = "x86_64", feature(abi_x86_interrupt))]

cfg_if::cfg_if! {
    if #[cfg(epoch_scheduler)] {
        extern crate scheduler_epoch as scheduler;
    } else if #[cfg(priority_scheduler)] {
        extern crate scheduler_priority as scheduler;
    } else {
        extern crate scheduler_round_robin as scheduler;
    }
}

use core::ops::Deref;

pub use scheduler::select_next_task;
use task_struct::RawTaskRef;

/// Changes the priority of the given task with the given priority level.
/// Priority values must be between 40 (maximum priority) and 0 (minimum
/// prriority). This function returns an error when a scheduler without priority
/// is loaded.
pub fn set_priority<T>(_task: &T, _priority: u8) -> Result<(), &'static str>
where
    T: Deref<Target = RawTaskRef>,
{
    #[cfg(any(epoch_scheduler, priority_scheduler))]
    {
        Ok(scheduler::set_priority(&_task, _priority))
    }
    #[cfg(not(any(epoch_scheduler, priority_scheduler)))]
    {
        Err("called set priority on scheduler that doesn't support set priority")
    }
}

/// Returns the priority of a given task.
/// This function returns None when a scheduler without priority is loaded.
pub fn get_priority<T>(_task: &T) -> Option<u8>
where
    T: Deref<Target = RawTaskRef>,
{
    #[cfg(any(epoch_scheduler, priority_scheduler))]
    {
        scheduler::get_priority(&_task)
    }
    #[cfg(not(any(epoch_scheduler, priority_scheduler)))]
    {
        None
    }
}

pub fn inherit_priority<T>(task: &T) -> scheduler::PriorityInheritanceGuard<'_>
where
    T: Deref<Target = RawTaskRef>,
{
    scheduler::inherit_priority(task)
}
