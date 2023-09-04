use alloc::{boxed::Box, vec::Vec};
use core::ptr;

use cpu::CpuId;
use log::error;
use spin::Mutex;

use crate::TaskRef;

/// List of all the schedulers on the system.
///
/// This is primarily used for spawning tasks, either to find the least busy CPU
/// or spawn a task pinned to a particular CPU.
static SCHEDULERS: Mutex<Vec<(CpuId, &'static ConcurrentScheduler)>> = Mutex::new(Vec::new());

/// A reference to the current CPUs scheduler.
///
/// This isn't strictly necessary, but it greatly improves performance, as it
/// avoids having to lock the system-wide list of schedulers.
#[cls::cpu_local]
static SCHEDULER: Option<&'static ConcurrentScheduler> = None;

type ConcurrentScheduler = Mutex<Box<dyn Scheduler>>;

/// Yields the current CPU by selecting a new `Task` to run next,
/// and then switches to that new `Task`.
///
/// The new "next" `Task` to run will be selected by the currently-active
/// scheduler policy.
///
/// Preemption will be disabled while this function runs,
/// but interrupts are not disabled because it is not necessary.
///
/// ## Return
/// * `true` if a new task was selected and switched to.
/// * `false` if no new task was selected, meaning the current task will
///   continue running.
#[doc(alias("yield"))]
pub fn schedule() -> bool {
    let preemption_guard = preemption::hold_preemption();
    // If preemption was not previously enabled (before we disabled it above),
    // then we shouldn't perform a task switch here.
    if !preemption_guard.preemption_was_enabled() {
        // trace!("Note: preemption was disabled on CPU {}, skipping scheduler.", cpu::current_cpu());
        return false;
    }

    let cpu_id = preemption_guard.cpu_id();

    let next_task = next_task();

    let (did_switch, recovered_preemption_guard) = super::task_switch(next_task, cpu_id, preemption_guard);

    // trace!("AFTER TASK_SWITCH CALL (CPU {}) new current: {:?}, interrupts are {}", cpu_id, task::get_my_current_task(), irq_safety::interrupts_enabled());

    drop(recovered_preemption_guard);
    did_switch
}

/// Sets the scheduler policy for the given CPU.
pub fn set_policy<T>(cpu_id: CpuId, scheduler: T)
where
    T: Scheduler,
{
    let boxed: Box<dyn Scheduler> = Box::new(scheduler);
    let mutex = Mutex::new(boxed);

    let scheduler_ref = {
        let ptr = Box::into_raw(Box::new(mutex));
        // SAFETY: We just converted the box into a raw pointer.
        unsafe { &*ptr }
    };

    let mut locked = SCHEDULERS.lock();
    SCHEDULER.update(|current_scheduler| {
        if let Some(old_scheduler) = current_scheduler {
            // FIXME: Drain tasks from old scheduler and place into new scheduler.
            error!("replacing existing scheduler: this is not currently supported");

            let mut old_scheduler_index = None;
            for (i, (cpu, scheduler)) in locked.iter().enumerate() {
                if *cpu == cpu_id {
                    if ptr::eq(old_scheduler, scheduler) {
                        old_scheduler_index = Some(i);
                        break;
                    } else {
                        panic!();
                    }
                }
            }

            if let Some(old_scheduler_index) = old_scheduler_index {
                locked.swap_remove(old_scheduler_index);
                // SAFETY: We just dropped the only other reference.
                let boxed = unsafe { Box::from_raw(old_scheduler) };
                drop(boxed);
            } else {
                // TODO: Log error.
                panic!();
            }
        }

        locked.push((cpu_id, scheduler_ref));
        *current_scheduler = Some(scheduler_ref);
    });
}

pub(crate) fn next_task() -> TaskRef {
    SCHEDULER.update(|scheduler| scheduler.unwrap().lock().as_mut().next())
}

pub fn add_task(task: TaskRef) {
    let locked = SCHEDULERS.lock();

    let max_busyness = usize::MAX;
    let mut least_busy_index = None;

    for (i, (_, scheduler)) in locked.iter().enumerate() {
        if scheduler.lock().busyness() < max_busyness {
            least_busy_index = Some(i);
        }
    }

    // TODO
    locked[least_busy_index.unwrap()].1.lock().add(task);
}

pub fn add_task_to(task: TaskRef, cpu_id: CpuId) {
    for (cpu, scheduler) in SCHEDULERS.lock().iter() {
        if *cpu == cpu_id {
            scheduler.lock().add(task);
            return;
        }
    }
}

pub fn add_task_to_current(task: TaskRef) {
    SCHEDULER.update(|scheduler| scheduler.unwrap().lock().add(task))
}

pub fn remove_task(task: &TaskRef) -> bool {
    for (_, scheduler) in SCHEDULERS.lock().iter() {
        if scheduler.lock().remove(task) {
            return true;
        }
    }
    false
}

pub fn remove_task_from(task: &TaskRef, cpu_id: CpuId) -> bool {
    for (cpu, scheduler) in SCHEDULERS.lock().iter() {
        if *cpu == cpu_id {
            return scheduler.lock().remove(task);
        }
    }
    false
}

pub fn remove_task_from_current(task: &TaskRef) -> bool {
    SCHEDULER.update(|scheduler| scheduler.unwrap().lock().remove(task))
}

pub trait Scheduler: Send + Sync + 'static {
    fn next(&mut self) -> TaskRef;

    fn add(&mut self, task: TaskRef);

    fn busyness(&self) -> usize;

    fn remove(&mut self, task: &TaskRef) -> bool;

    fn as_priority_scheduler(&mut self) -> Option<&mut dyn PriorityScheduler>;
}

pub trait PriorityScheduler {
    fn set_priority(&mut self, task: &TaskRef);

    fn get_priority(&mut self, task: &TaskRef);

    fn inherit_priority(&mut self, task: &TaskRef);
}
