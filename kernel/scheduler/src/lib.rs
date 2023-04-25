//! Offers the ability to control or configure the active task scheduling policy.
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

extern crate alloc;

use cpu::CpuId;
use interrupts::{self, CPU_LOCAL_TIMER_IRQ, eoi};
use scheduler_round_robin::SchedulerRoundRobin;
use task::{self, TaskRef};

/// A re-export of [`task::schedule()`] for convenience and legacy compatibility.
pub use task::schedule;

pub use current_scheduler_policy::{current_scheduler, set_current_scheduler};
mod current_scheduler_policy {
    use crossbeam_utils::atomic::AtomicCell;
    use scheduler_policy::SchedulerPolicy;
    use scheduler_round_robin::SchedulerRoundRobin;

    static CURRENT_SCHEDULER: AtomicCell<&SchedulerRoundRobin> =
        AtomicCell::new(&SchedulerRoundRobin);
    const _: () = assert!(AtomicCell::<&SchedulerRoundRobin>::is_lock_free());

    pub fn current_scheduler() -> &'static SchedulerRoundRobin {
        CURRENT_SCHEDULER.load()
    }

    pub fn set_current_scheduler(new_policy: &SchedulerRoundRobin) {
        task::set_scheduler_policy(|cpu_id| new_policy.select_next_task(cpu_id));
        CURRENT_SCHEDULER.store(new_policy);
    }
}

/// Initializes the scheduler on this CPU using the policy set at compiler time.
///
/// Also registers a timer interrupt handler for preemptive scheduling.
///
/// Currently, there is a single scheduler policy for the whole system.
/// The policy is selected by specifying a Rust `cfg` value at build time, like so:
/// * `make` --> basic round-robin scheduler, the default.
/// * `make THESEUS_CONFIG=priority_scheduler` --> priority scheduler.
/// * `make THESEUS_CONFIG=realtime_scheduler` --> "realtime" (rate monotonic) scheduler.
pub fn init(cpu: CpuId) -> Result<(), &'static str> {
    // TODO: temporary fix this
    // task::set_scheduler_policy(scheduler::select_next_task);

    #[cfg(target_arch = "x86_64")] {
        interrupts::register_interrupt(
            CPU_LOCAL_TIMER_IRQ,
            lapic_timer_handler,
        ).map_err(|_handler| {
            log::error!("BUG: interrupt {CPU_LOCAL_TIMER_IRQ} was already registered to handler {_handler:#X}");
            "BUG: CPU-local timer interrupt was already registered to a handler"
        })
    }

    #[cfg(target_arch = "aarch64")] {
        interrupts::init_timer(aarch64_timer_handler)?;
        interrupts::enable_timer(true);
        Ok(())
    }
}

/// The handler for each CPU's local timer interrupt, used for preemptive task switching.
#[cfg(target_arch = "aarch64")]
extern "C" fn aarch64_timer_handler(_exc: &interrupts::ExceptionContext) -> interrupts::EoiBehaviour {
    interrupts::schedule_next_timer_tick();
    cpu_local_timer_tick_handler();
    interrupts::EoiBehaviour::HandlerHasSignaledEoi
}

/// The handler for each CPU's local timer interrupt, used for preemptive task switching.
#[cfg(target_arch = "x86_64")]
extern "x86-interrupt" fn lapic_timer_handler(_stack_frame: x86_64::structures::idt::InterruptStackFrame) {
    cpu_local_timer_tick_handler()
}

// Cross platform scheduling code
fn cpu_local_timer_tick_handler() {
    // tick count, only used for debugging
    if false {
        use core::sync::atomic::{AtomicUsize, Ordering};
        static CPU_LOCAL_TIMER_TICKS: AtomicUsize = AtomicUsize::new(0);
        let _ticks = CPU_LOCAL_TIMER_TICKS.fetch_add(1, Ordering::Relaxed);
        log::info!("(CPU {}) CPU-LOCAL TIMER HANDLER! TICKS = {}", cpu::current_cpu(), _ticks);
    }

    // Inform the `sleep` crate that it should update its inner tick count
    // in order to unblock any tasks that are done sleeping.
    sleep::unblock_sleeping_tasks();

    // We must acknowledge the interrupt before the end of this handler
    // because we switch tasks here, which doesn't return.
    {
        #[cfg(target_arch = "x86_64")]
        eoi(None); // None, because IRQ 0x22 cannot possibly be a PIC interrupt

        #[cfg(target_arch = "aarch64")]
        eoi(CPU_LOCAL_TIMER_IRQ);
    }

    schedule();
}

/// Changes the priority of the given task with the given priority level.
/// Priority values must be between 40 (maximum priority) and 0 (minimum prriority).
/// This function returns an error when a scheduler without priority is loaded. 
pub fn set_priority(_task: &TaskRef, _priority: u8) -> Result<(), &'static str> {
    // #[cfg(priority_scheduler)] {
    //     scheduler_priority::set_priority(_task, _priority)
    // }
    #[cfg(not(priority_scheduler))] {
        Err("no scheduler that uses task priority is currently loaded")
    }
}

/// Returns the priority of a given task.
/// This function returns None when a scheduler without priority is loaded.
pub fn get_priority(_task: &TaskRef) -> Option<u8> {
    // #[cfg(priority_scheduler)] {
    //     scheduler_priority::get_priority(_task)
    // }
    #[cfg(not(priority_scheduler))] {
        None
    }
}

pub fn set_periodicity(_task: &TaskRef, _period: usize) -> Result<(), &'static str> {
    // #[cfg(realtime_scheduler)] {
    //     scheduler_realtime::set_periodicity(_task, _period)
    // }
    #[cfg(not(realtime_scheduler))] {
        Err("no scheduler that supports periodic tasks is currently loaded")
    }
}
