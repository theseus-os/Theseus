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

use interrupts::{self, CPU_LOCAL_TIMER_IRQ, interrupt_handler, eoi, EoiBehaviour};

/// Re-exports for convenience and legacy compatibility.
pub use task::scheduler::{inherit_priority, priority, schedule, set_priority};


/// Initializes the scheduler on this system using the policy set at compiler time.
///
/// Also registers a timer interrupt handler for preemptive scheduling.
///
/// Currently, there is a single scheduler policy for the whole system.
/// The policy is selected by specifying a Rust `cfg` value at build time, like so:
/// - `make`: round-robin scheduler
/// - `make THESEUS_CONFIG=epoch_scheduler`: epoch scheduler
/// - `make THESEUS_CONFIG=priority_scheduler`: priority scheduler
pub fn init() -> Result<(), &'static str> {
    #[cfg(target_arch = "x86_64")] {
        interrupts::register_interrupt(
            CPU_LOCAL_TIMER_IRQ,
            timer_tick_handler,
        ).map_err(|_handler| {
            log::error!("BUG: interrupt {CPU_LOCAL_TIMER_IRQ} was already registered to handler {_handler:#X}");
            "BUG: CPU-local timer interrupt was already registered to a handler"
        })
    }

    #[cfg(target_arch = "aarch64")] {
        interrupts::setup_timer_interrupt(timer_tick_handler)?;
        generic_timer_aarch64::enable_timer_interrupt(true);
        Ok(())
    }
}

// Architecture-independent timer interrupt handler for preemptive scheduling.
interrupt_handler!(timer_tick_handler, _, _stack_frame, {
    #[cfg(target_arch = "aarch64")]
    generic_timer_aarch64::set_next_timer_interrupt(get_timeslice_ticks());

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

    // We must acknowledge the interrupt *before* the end of this handler
    // because we switch tasks here, which doesn't return.
    eoi(CPU_LOCAL_TIMER_IRQ);

    schedule();

    EoiBehaviour::HandlerSentEoi
});


/// Returns the (cached) number of system timer ticks needed for the scheduling timeslice interval.
///
/// This is only needed on aarch64 because it only effectively offers a one-shot timer;
/// x86_64 can be configured once as a recurring periodic timer.
#[cfg(target_arch = "aarch64")]
fn get_timeslice_ticks() -> u64 {
    use kernel_config::time::CONFIG_TIMESLICE_PERIOD_MICROSECONDS;

    static TIMESLICE_TICKS: spin::Once<u64> = spin::Once::new();

    *TIMESLICE_TICKS.call_once(|| {
        let timeslice_femtosecs = (CONFIG_TIMESLICE_PERIOD_MICROSECONDS as u64) * 1_000_000_000;
        let tick_period_femtosecs = generic_timer_aarch64::timer_period_femtoseconds();
        timeslice_femtosecs / tick_period_femtosecs
    })
}
