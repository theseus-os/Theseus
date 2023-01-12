#![no_std]

extern crate alloc;

mod imp;
mod queue;

use crate::queue::RunQueue;
use atomic_linked_list::atomic_map::AtomicMap;
use log::error;
use mutex_preemption::RwLockPreempt;
use task::TaskRef;

pub use imp::{get_priority, set_periodicity, set_priority};

// TODO: Is RwLockPreempt necessary? We already hold_preemption when we enter
// the schedule function.
static RUN_QUEUES: AtomicMap<u8, RwLockPreempt<RunQueue>> = AtomicMap::new();

pub fn init(core: u8, idle_task: TaskRef) -> Result<(), &'static str> {
    #[cfg(runqueue_spillful)]
    {
        task::RUNQUEUE_REMOVAL_FUNCTION.call_once(|| remove_task_from_within_task);
    }

    if RUN_QUEUES
        .insert(core, RwLockPreempt::new(RunQueue::new(core, idle_task)))
        .is_some()
    {
        Err("run queue already exists for this core")
    } else {
        Ok(())
    }
}

pub fn get_run_queue(core: u8) -> Option<&'static RwLockPreempt<RunQueue>> {
    RUN_QUEUES.get(&core)
}

pub fn add_task_to_any_run_queue(task: TaskRef) -> Result<(), &'static str> {
    let locked = get_least_busy_run_queue().ok_or("no run queues to add task to")?;
    let mut unlocked = locked.write();
    unlocked.add(task);
    Ok(())
}

pub fn add_task_to_specific_run_queue(core: u8, task: TaskRef) -> Result<(), &'static str> {
    RUN_QUEUES
        .get(&core)
        .ok_or("run queue does not exist for core")?
        .write()
        .add(task);
    Ok(())
}

pub fn remove_task_from_all(task: &TaskRef) {
    for (_, run_queue) in RUN_QUEUES.iter() {
        run_queue.write().remove(task);
    }
}

fn get_least_busy_run_queue() -> Option<&'static RwLockPreempt<RunQueue>> {
    let mut min_rq: Option<(&'static RwLockPreempt<RunQueue>, usize)> = None;

    for (_, rq) in RUN_QUEUES.iter() {
        let rq_size = rq.read().len();

        if let Some(min) = min_rq {
            if rq_size < min.1 {
                min_rq = Some((rq, rq_size));
            }
        } else {
            min_rq = Some((rq, rq_size));
        }
    }

    log::info!("THING: {min_rq:#?}");
    min_rq.map(|m| m.0)
}

#[cfg(runqueue_spillful)]
/// Removes a `TaskRef` from the RunQueue(s) on the given `core`.
/// Note: This method is only used by the state spillful runqueue
/// implementation.
pub fn remove_task_from_within_task(task: &TaskRef, core: u8) -> Result<(), &'static str> {
    task.set_on_runqueue(None);
    RUN_QUEUES
        .get(&core)
        .ok_or("Couldn't get runqueue for specified core")
        .and_then(|rq| {
            // Instead of calling `remove_task`, we directly call `remove_internal`
            // because we want to actually remove the task from the runqueue,
            // as calling `remove_task` would do nothing due to it skipping the actual
            // removal when the `runqueue_spillful` cfg is enabled.
            rq.write().remove_internal(task)
        })
}

/// Yields the current CPU by selecting a new `Task` to run
/// and then switching to that new `Task`.
///
/// Preemption will be disabled while this function runs,
/// but interrupts are not disabled because it is not necessary.
///
/// ## Return
/// * `true` if a new task was selected and switched to.
/// * `false` if no new task was selected, meaning the current task will
///   continue running.
pub fn schedule() -> bool {
    let preemption_guard = preemption::hold_preemption();
    // If preemption was not previously enabled (before we disabled it above),
    // then we shouldn't perform a task switch here.
    if !preemption_guard.preemption_was_enabled() {
        return false;
    }

    let apic_id = preemption_guard.cpu_id();
    let mut run_queue = match RUN_QUEUES.get(&apic_id) {
        Some(rq) => rq.write(),
        _ => {
            error!(
                "BUG: schedule(): couldn't get run queue for core {}",
                apic_id
            );
            return false;
        }
    };

    let next_task = run_queue.next();
    drop(run_queue);

    let (did_switch, recovered_preemption_guard) =
        task::task_switch(next_task, apic_id, preemption_guard);

    drop(recovered_preemption_guard);
    did_switch
}
