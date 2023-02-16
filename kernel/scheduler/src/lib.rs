#![no_std]

extern crate alloc;

mod queue;
mod imp {
    cfg_if::cfg_if! {
        if #[cfg(priority_scheduler)] {
            pub(crate) use scheduler_priority::*;
        } else if #[cfg(realtime_scheduler)] {
            pub(crate) use scheduler_realtime::*;
        } else {
            pub(crate) use scheduler_round_robin::*;
        }
    }
}

use crate::queue::RunQueue;
use atomic_linked_list::atomic_map::AtomicMap;
use log::error;
use mutex_preemption::RwLockPreempt;
use task::TaskRef;

static RUN_QUEUES: AtomicMap<u8, RwLockPreempt<RunQueue>> = AtomicMap::new();

pub fn init(core: u8, idle_task: TaskRef) -> Result<(), &'static str> {
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

pub fn get_least_busy_run_queue() -> Option<&'static RwLockPreempt<RunQueue>> {
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

    min_rq.map(|m| m.0)
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

    let next_task = {
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
        run_queue.next()
    };

    let (did_switch, recovered_preemption_guard) =
        task::task_switch(next_task, apic_id, preemption_guard);

    drop(recovered_preemption_guard);
    did_switch
}

pub fn get_priority(task: &TaskRef) -> Option<u8> {
    for (_, queue) in RUN_QUEUES.iter() {
        if let Some(priority) = queue.read().get_priority(task) {
            return Some(priority);
        }
    }

    None
}

pub fn set_priority(task: &TaskRef, priority: u8) -> Result<(), &'static str> {
    for (_, queue) in RUN_QUEUES.iter() {
        queue.write().set_priority(task, priority)?;
    }

    Ok(())
}

pub fn set_periodicity(task: &TaskRef, periodicity: usize) -> Result<(), &'static str> {
    for (_, queue) in RUN_QUEUES.iter() {
        queue.write().set_periodicity(task, periodicity)?;
    }

    Ok(())
}
