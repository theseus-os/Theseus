//! Offers the ability to control or configure the active task scheduling policy.
//!
//! Note that actual task switching and preemptive scheduling are implemented
//! in the [`task`] crate.
//! This crate re-exports that main [`schedule()`] function for convenience,
//! legacy compatbility, and to act as an easy landing page for code search.
//! That means that a caller need only depend on [`task`], not this crate,
//! to invoke the scheduler (yield the CPU) to switch to another task.

#![no_std]
#![cfg_attr(target_arch = "x86_64", feature(abi_x86_interrupt))]

cfg_if::cfg_if! {
    if #[cfg(priority_scheduler)] {
        extern crate scheduler_priority as scheduler;
    } else if #[cfg(realtime_scheduler)] {
        extern crate scheduler_realtime as scheduler;
    } else {
        extern crate scheduler_round_robin as scheduler;
    }
}

use interrupts::{CPU_LOCAL_TIMER_IRQ, eoi, register_interrupt};
use task::TaskRef;

/// A re-export of [`task::schedule()`] for convenience and legacy compatibility.
pub use task::schedule;


/// Initializes the scheduler on this system using the policy set at compiler time.
///
/// Also registers a timer interrupt handler for preemptive scheduling.
///
/// Currently, there is a single scheduler policy for the whole system.
/// The policy is selected by specifying a Rust `cfg` value at build time, like so:
/// * `make THESEUS_CONFIG=priority_scheduler` --> priority scheduler.
/// * `make THESEUS_CONFIG=realtime_scheduler` --> "realtime" (rate monotonic) scheduler.
/// * `make` --> basic round-robin scheduler, the default.
pub fn init() -> Result<(), &'static str> {
    task::set_scheduler_policy(scheduler::select_next_task);

    #[cfg(target_arch = "x86_64")] {
        register_interrupt(
            CPU_LOCAL_TIMER_IRQ,
            lapic_timer_handler,
        ).map_err(|_handler| {
            log::error!("BUG: interrupt {CPU_LOCAL_TIMER_IRQ} was already registered to handler {_handler:#X}");
            "BUG: CPU-local timer interrupt was already registered to a handler"
        })
    }

    #[cfg(not(target_arch = "x86_64"))] {
        log::error!("TODO: scheduler::init() only supports registering a preemptive task switching timer interrupt on x86_64");
        Err("TODO: scheduler::init() only supports registering a preemptive task switching timer interrupt on x86_64")
    }
}

/// The handler for each CPU's local timer interrupt, used for preemptive task switching.
#[cfg(target_arch = "x86_64")]
extern "x86-interrupt" fn lapic_timer_handler(_stack_frame: x86_64::structures::idt::InterruptStackFrame) {
    // tick count, only used for debugging
    #[cfg(any())] { // cfg(any()) is always false
        use core::sync::atomic::{AtomicUsize, Ordering};
        static LAPIC_TIMER_TICKS: AtomicUsize = AtomicUsize::new(0);
        let _ticks = LAPIC_TIMER_TICKS.fetch_add(1, Ordering::Relaxed);
        log::info!("(CPU {}) LAPIC TIMER HANDLER! TICKS = {}", cpu::current_cpu(), _ticks);
    }

    // Inform the `sleep` crate that it should update its inner tick count
    // in order to unblock any tasks that are done sleeping.
    sleep::unblock_sleeping_tasks();

    // We must acknowledge the interrupt before the end of this handler
    // because we switch tasks here, which doesn't return.
    eoi(None); // None, because IRQ 0x22 cannot possibly be a PIC interrupt

    schedule();
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
