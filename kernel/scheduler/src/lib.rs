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
#![feature(trait_alias)]
#![cfg_attr(target_arch = "x86_64", feature(abi_x86_interrupt))]

extern crate alloc;

use alloc::boxed::Box;
use scheduler_policy::RunqueueId;
use core::ops::Deref;
use cpu::CpuId;
use cpu_local_preemption::{CpuLocal, CpuLocalField, PerCpuField, PreemptionGuard};
use interrupts::{CPU_LOCAL_TIMER_IRQ, eoi};
use task::{TaskRef, ExitableTaskRef};

pub use scheduler_policy::*;

/// A re-export of [`task::schedule()`] for convenience and legacy compatibility.
pub use task::schedule;

/// A scheduler policy that can be stored in a static variable.
pub trait StaticSchedulerPolicy = SchedulerPolicy + Send + Sync + 'static;

/// The scheduler policy in use for the current CPU.
static CURRENT_SCHEDULER_POLICY: CpuLocal<BoxedSchedulerPolicy> =
    CpuLocal::new(PerCpuField::BoxedSchedulerPolicy);

/// Sets the scheduler policy on this CPU.
///
/// Note that this must be called individually on each CPU that you want
/// the given `new_policy` to be used.
fn set_current_scheduler_policy<P: StaticSchedulerPolicy>(new_policy: P) {
    // With the current design, we don't need to repeatedly set this callback,
    // we only need to do it once per CPU.
    // task::set_scheduler_policy(select_next_task_callback);

    CURRENT_SCHEDULER_POLICY.with_mut(
        |sched| *sched = BoxedSchedulerPolicy::new(new_policy)
    );
}

/// A callback that can be registered with the `task` crate for scheduling.
fn select_next_task_callback(cpu_id: CpuId, guard: &PreemptionGuard) -> Option<TaskRef> {
    CURRENT_SCHEDULER_POLICY.with_preempt(
        guard,
        |sched| sched.select_next_task(cpu_id.into()),
    )
}

pub fn with_scheduler<F, R>(func: F) -> R
where
    F: FnOnce(&BoxedSchedulerPolicy) -> R
{
    CURRENT_SCHEDULER_POLICY.with(func)
}

/// A scheduler policy boxed up as a trait object.
///
/// The [`SchedulerPolicy`] trait is forwarded through this struct.
pub struct BoxedSchedulerPolicy(Box<dyn StaticSchedulerPolicy>);
impl BoxedSchedulerPolicy {
    /// Boxes up the given `scheduler_policy` as a trait object within this struct.
    pub fn new<P: StaticSchedulerPolicy>(scheduler_policy: P) -> Self {
        Self(Box::new(scheduler_policy))
    }
}
impl SchedulerPolicy for BoxedSchedulerPolicy {
    fn init_runqueue(&self, rq_id: RunqueueId) -> Result<(), RunqueueError> {
        self.0.init_runqueue(rq_id)
    }
    fn select_next_task(&self, rq_id: RunqueueId) -> Option<TaskRef> {
        self.0.select_next_task(rq_id)
    }
    fn add_task(&self, task: TaskRef, rq_id: Option<RunqueueId>) -> Result<(), RunqueueError> {
        self.0.add_task(task, rq_id)
    }
    fn remove_task(&self, task: &TaskRef) -> Result<(), RunqueueError> {
        self.0.remove_task(task)
    }
    fn runqueue_iter(&self) -> scheduler_policy::AllRunqueuesIterator {
        self.0.runqueue_iter()
    }
}
// SAFETY: The `BoxedSchedulerPolicy` type corresponds to a field in `PerCpuData`
//         with the offset specified by `PerCpuField::BoxedSchedulerPolicy`.
unsafe impl CpuLocalField for BoxedSchedulerPolicy {
    const FIELD: PerCpuField = PerCpuField::BoxedSchedulerPolicy;
}


/// Initializes the scheduler on this CPU with the scheduler policy selected at compiler time.
///
/// Also registers a timer interrupt handler for preemptive scheduling.
///
/// ## Arguments
/// * `cpu`: the CPU on which this init routine is running.
/// * `bootstrap_task`: a reference to the currently-running task,
///    which was bootstrapped from this `cpu`'s initial thread of execution
///    and will be added to this scheduler's runqueue for this `cpu`.
///
/// ## Scheduler Policy behavior
/// Currently, a single scheduler policy is set for the whole system by default,
/// but it can be explicitly set on each CPU as desired.
/// The policy is selected by specifying a Rust `cfg` value at build time, like so:
/// * `make` --> basic round-robin scheduler, the default.
/// * `make THESEUS_CONFIG=priority_scheduler` --> priority scheduler.
/// * `make THESEUS_CONFIG=realtime_scheduler` --> "realtime" (rate monotonic) scheduler.
pub fn init(cpu: CpuId, bootstrap_task: &ExitableTaskRef) -> Result<(), &'static str> {
    // This must be done on every CPU.
    let scheduler_policy = {
        cfg_if::cfg_if! {
            if #[cfg(priority_scheduler)] {
                scheduler_priority::SchedulerPriority
            } else if #[cfg(realtime_scheduler)] {
                scheduler_realtime::SchedulerRealtime
            } else {
                scheduler_round_robin::SchedulerRoundRobin::new()
            }
        }
    };

    scheduler_policy.add_task(bootstrap_task.deref().clone(), Some(cpu.into()))
        .map_err(RunqueueError::into_static_str)?;
    set_current_scheduler_policy(BoxedSchedulerPolicy::new(scheduler_policy));

    // The rest of this function only needs to be done once, system-wide.
    if !cpu.is_bootstrap_cpu() {
        return Ok(());
    }

    task::set_scheduler_policy(select_next_task_callback);

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
