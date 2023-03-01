//! Offers the ability to control or configure the active task scheduling policy.
//!
//! Note that actual task switching and preemptive scheduling are actually
//! implemented in the [`task`] crate.
//! This crate re-exports that main [`schedule()`] function for convenience,
//! legacy compatbility, and to act as an easy landing page for code search.
//! That means that a caller need only depend on [`task`], not this crate,
//! to invoke the scheduler (yield the CPU) to switch to another task.

#![no_std]

cfg_if::cfg_if! {
    if #[cfg(priority_scheduler)] {
        extern crate scheduler_priority as scheduler;
    } else if #[cfg(realtime_scheduler)] {
        extern crate scheduler_realtime as scheduler;
    } else {
        extern crate scheduler_round_robin as scheduler;
    }
}

use task::TaskRef;

/// A re-export of [`task::schedule()`] for convenience and legacy compatibility.
pub use task::schedule;

/// Initializes the scheduler on this system using the policy set at compiler time.
///
/// Currently, there is a single scheduler policy for the whole system.
/// The policy is selected by specifying a Rust `cfg` value at build time, like so:
/// * `make THESEUS_CONFIG=priority_scheduler` --> priority scheduler.
/// * `make THESEUS_CONFIG=realtime_scheduler` --> "realtime" (rate monotonic) scheduler.
/// * `make` --> basic round-robin scheduler, the default.
pub fn init() {
    task::set_scheduler_policy(scheduler::select_next_task);
}

/// Changes the priority of the given task with the given priority level.
/// Priority values must be between 40 (maximum priority) and 0 (minimum prriority).
/// This function returns an error when a scheduler without priority is loaded. 
pub fn set_priority(_task: &TaskRef, _priority: u8) -> Result<(), &'static str> {
    #[cfg(priority_scheduler)] {
        scheduler_priority::set_priority(_task, _priority)
    }
    #[cfg(not(priority_scheduler))] {
        Err("no scheduler that uses task priority is currently loaded")
    }
}

/// Returns the priority of a given task.
/// This function returns None when a scheduler without priority is loaded.
pub fn get_priority(_task: &TaskRef) -> Option<u8> {
    #[cfg(priority_scheduler)] {
        scheduler_priority::get_priority(_task)
    }
    #[cfg(not(priority_scheduler))] {
        None
    }
}

pub fn set_periodicity(_task: &TaskRef, _period: usize) -> Result<(), &'static str> {
    #[cfg(realtime_scheduler)] {
        scheduler_realtime::set_periodicity(_task, _period)
    }
    #[cfg(not(realtime_scheduler))] {
        Err("no scheduler that supports periodic tasks is currently loaded")
    }
}
