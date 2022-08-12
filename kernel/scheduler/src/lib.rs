#![no_std]

extern crate alloc;
#[macro_use] extern crate log;
extern crate irq_safety;
extern crate apic;
extern crate task;
extern crate runqueue;
#[macro_use] extern crate cfg_if;
cfg_if! {
    if #[cfg(priority_scheduler)] {
        extern crate scheduler_priority as scheduler;
    } else if #[cfg(realtime_scheduler)] {
        extern crate scheduler_realtime as scheduler;
    } else {
        extern crate scheduler_round_robin as scheduler;
    }
}

use core::ops::Deref;
use irq_safety::hold_interrupts;
use apic::get_my_apic_id;
use task::{get_my_current_task, TaskRef};

/// Yields the current CPU by selecting a new `Task` to run 
/// and then performs a task switch to that new `Task`.
///
/// Interrupts will be disabled while this function runs.
pub fn schedule() -> bool {
    let _held_interrupts = hold_interrupts(); // auto-reenables interrupts on early return
    let apic_id = get_my_apic_id();

    let curr_task = if let Some(curr) = get_my_current_task() {
        curr
    } else {
        error!("BUG: schedule(): could not get current task.");
        return false; // keep running the same current task
    };

    let next_task = if let Some(next) = scheduler::select_next_task(apic_id) {
        next
    } else {
        return false; // keep running the same current task
    };

    // No need to task switch if the chosen task is the same as the current task.
    if curr_task == &next_task {
        return false;
    }

    // trace!("BEFORE TASK_SWITCH CALL (AP {}), current: {:?}, next: {:?}, interrupts are {}", apic_id, curr_task, next_task, irq_safety::interrupts_enabled());

    curr_task.task_switch(next_task.deref(), apic_id); 

    // trace!("AFTER TASK_SWITCH CALL (AP {}) new current: {:?}, interrupts are {}", apic_id, get_my_current_task(), irq_safety::interrupts_enabled());
 
    true
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
