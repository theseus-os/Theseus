use alloc::{boxed::Box, sync::Arc, vec::Vec};
use core::ptr;

use cpu::CpuId;
use spin::Mutex;
use sync_preemption::PreemptionSafeMutex;

use crate::TaskRef;

/// List of all the schedulers on the system.
///
/// This is primarily used for spawning tasks, either to find the least busy CPU
/// or spawn a task pinned to a particular CPU.
///
/// The outer mutex does not need to be preemption-safe, because it is never
/// accessed from `schedule`. In fact, ideally it would be a blocking mutex, but
/// that leads to circular dependencies.
static SCHEDULERS: Mutex<Vec<(CpuId, Arc<ConcurrentScheduler>)>> = Mutex::new(Vec::new());

/// A reference to the current CPUs scheduler.
///
/// This isn't strictly necessary, but it greatly improves performance, as it
/// avoids having to lock the system-wide list of schedulers.
#[cls::cpu_local]
static SCHEDULER: Option<Arc<ConcurrentScheduler>> = None;

type ConcurrentScheduler = PreemptionSafeMutex<Box<dyn Scheduler>>;

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

    let next_task =
        SCHEDULER.update(|scheduler| scheduler.as_ref().unwrap().lock().as_mut().next());

    let (did_switch, recovered_preemption_guard) =
        super::task_switch(next_task, cpu_id, preemption_guard);

    // log::trace!("AFTER TASK_SWITCH CALL (CPU {}) new current: {:?}, interrupts are {}", cpu_id, super::get_my_current_task(), irq_safety::interrupts_enabled());

    drop(recovered_preemption_guard);
    did_switch
}

/// Sets the scheduler policy for the given CPU.
pub fn set_policy<T>(cpu_id: CpuId, scheduler: T)
where
    T: Scheduler,
{
    let boxed: Box<dyn Scheduler> = Box::new(scheduler);
    let mutex = PreemptionSafeMutex::new(boxed);
    let scheduler = Arc::new(mutex);

    let mut locked = SCHEDULERS.lock();
    SCHEDULER.update(|current_scheduler| {
        if let Some(old_scheduler) = current_scheduler {
            let mut old_scheduler_index = None;
            for (i, (cpu, scheduler)) in locked.iter().enumerate() {
                if *cpu == cpu_id {
                    debug_assert!(ptr::eq(old_scheduler, scheduler));
                    old_scheduler_index = Some(i);
                    break;
                }
            }

            if let Some(old_scheduler_index) = old_scheduler_index {
                locked.swap_remove(old_scheduler_index);
            } else {
                log::error!("BUG: current scheduler not found in `SCHEDULERS`");
            }

            let mut new_scheduler = scheduler.lock();
            for task in old_scheduler.lock().drain() {
                new_scheduler.add(task);
            }
        }

        locked.push((cpu_id, scheduler.clone()));
        *current_scheduler = Some(scheduler);
    });
}

/// Adds the given task to the least busy run queue.
pub fn add_task(task: TaskRef) {
    let locked = SCHEDULERS.lock();

    let mut min_busyness = usize::MAX;
    let mut least_busy_index = None;

    for (i, (_, scheduler)) in locked.iter().enumerate() {
        let busyness = scheduler.lock().busyness();
        if busyness < min_busyness {
            least_busy_index = Some(i);
            min_busyness = busyness;
        }
    }

    locked[least_busy_index.unwrap()].1.lock().add(task);
}

/// Adds the given task to the specified CPU's run queue.
pub fn add_task_to(cpu_id: CpuId, task: TaskRef) {
    for (cpu, scheduler) in SCHEDULERS.lock().iter() {
        if *cpu == cpu_id {
            scheduler.lock().add(task);
            return;
        }
    }
}

/// Adds the given task to the current CPU's run queue.
pub fn add_task_to_current(task: TaskRef) {
    SCHEDULER.update(|scheduler| scheduler.as_ref().unwrap().lock().add(task))
}

/// Removes the given task from all run queues.
pub fn remove_task(task: &TaskRef) -> bool {
    for (_, scheduler) in SCHEDULERS.lock().iter() {
        if scheduler.lock().remove(task) {
            // A task will only be on one run queue.
            return true;
        }
    }
    false
}

/// Removes the given task from the specified CPU's run queue.
pub fn remove_task_from(task: &TaskRef, cpu_id: CpuId) -> bool {
    for (cpu, scheduler) in SCHEDULERS.lock().iter() {
        if *cpu == cpu_id {
            return scheduler.lock().remove(task);
        }
    }
    false
}

/// Removes the given task from the current CPU's run queue.
pub fn remove_task_from_current(task: &TaskRef) -> bool {
    SCHEDULER.update(|scheduler| scheduler.as_ref().unwrap().lock().remove(task))
}

/// A task scheduler.
pub trait Scheduler: Send + Sync + 'static {
    /// Returns the next task to run.
    fn next(&mut self) -> TaskRef;

    /// Adds a task to the run queue.
    fn add(&mut self, task: TaskRef);

    /// Returns a measure of how busy the scheduler is.
    fn busyness(&self) -> usize;

    /// Removes a task from the run queue.
    fn remove(&mut self, task: &TaskRef) -> bool;

    /// Returns the scheduler as a priority scheduler, if it is one.
    fn as_priority_scheduler(&mut self) -> Option<&mut dyn PriorityScheduler>;

    /// Clears the scheduler, returning all contained tasks as an iterator.
    fn drain(&mut self) -> Box<dyn Iterator<Item = TaskRef> + '_>;

    /// Returns a list of contained tasks.
    ///
    /// The list should be considered out-of-date as soon as it is called, but
    /// can be useful as a heuristic.
    fn dump(&self) -> Vec<TaskRef>;
}

/// A task scheduler with some notion of priority.
pub trait PriorityScheduler {
    /// Sets the priority of the given task.
    fn set_priority(&mut self, task: &TaskRef, priority: u8) -> bool;

    /// Gets the priority of the given task.
    fn priority(&mut self, task: &TaskRef) -> Option<u8>;
}

/// Returns the priority of the given task.
///
/// Returns `None` if the task is not on a priority run queue.
pub fn priority(task: &TaskRef) -> Option<u8> {
    for (_, scheduler) in SCHEDULERS.lock().iter() {
        if let Some(priority) = scheduler
            .lock()
            .as_priority_scheduler()
            .and_then(|priority_scheduler| priority_scheduler.priority(task))
        {
            return Some(priority);
        }
    }
    None
}

/// Sets the priority of the given task.
///
/// Returns `None` if the task is not on a priority run queue.
pub fn set_priority(task: &TaskRef, priority: u8) -> bool {
    for (_, scheduler) in SCHEDULERS.lock().iter() {
        if let Some(true) = scheduler
            .lock()
            .as_priority_scheduler()
            .map(|priority_scheduler| priority_scheduler.set_priority(task, priority))
        {
            return true;
        }
    }
    false
}

/// Returns the busyness of the scheduler on the given CPU.
pub fn busyness(cpu_id: CpuId) -> Option<usize> {
    for (cpu, scheduler) in SCHEDULERS.lock().iter() {
        if *cpu == cpu_id {
            return Some(scheduler.lock().busyness());
        }
    }
    None
}

/// Modifies the given task's priority to be the maximum of its priority and the
/// current task's priority.
///
/// Returns a guard which reverts the change when dropped.
pub fn inherit_priority(task: &TaskRef) -> PriorityInheritanceGuard<'_> {
    let current_priority = super::with_current_task(priority).unwrap();
    let other_priority = priority(task);

    if let (Some(current_priority), Some(other_priority)) =
        (current_priority, other_priority) && current_priority > other_priority
    {
        set_priority(task, current_priority);
    }

    PriorityInheritanceGuard {
        inner: if let (Some(current_priority), Some(other_priority)) =
            (current_priority, other_priority)
            && current_priority > other_priority
        {
            Some((task, other_priority))
        } else {
            None
        },
    }
}

/// Lowers the task's priority to its previous value when dropped.
pub struct PriorityInheritanceGuard<'a> {
    inner: Option<(&'a TaskRef, u8)>,
}

impl<'a> Drop for PriorityInheritanceGuard<'a> {
    fn drop(&mut self) {
        if let Some((task, priority)) = self.inner {
            set_priority(task, priority);
        }
    }
}

/// Returns the list of tasks running on each CPU.
///
/// To avoid race conditions with migrating tasks, this function takes a lock
/// over all system schedulers. This is incredibly disruptive and should be
/// avoided at all costs.
pub fn dump() -> Vec<(CpuId, Vec<TaskRef>)> {
    let schedulers = SCHEDULERS.lock().clone();
    let locked = schedulers
        .iter()
        .map(|(cpu, scheduler)| (cpu, scheduler.lock()))
        // We eagerly evaluate so that all schedulers are actually locked.
        .collect::<Vec<_>>();
    let result = locked
        .iter()
        .map(|(cpu, locked_scheduler)| (**cpu, locked_scheduler.dump()))
        .collect();
    drop(locked);
    result
}
