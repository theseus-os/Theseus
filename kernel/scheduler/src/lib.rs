#![no_std]

mod imp;

use atomic_linked_list::atomic_map::AtomicMap;
use log::error;
use mutex_preemption::RwLockPreempt;
use runqueue::RunQueue;

pub use imp::{get_priority, set_periodicity, set_priority};

// TODO: Is RwLockPreempt necessary? We already hold_preemption when we enter
// the schedule function.
static RUN_QUEUES: AtomicMap<u8, RwLockPreempt<RunQueue<imp::TaskRef>>> = AtomicMap::new();

pub fn init(core: u8) -> Result<(), &'static str> {
    if RUN_QUEUES
        .insert(core, RwLockPreempt::new(RunQueue::new(core)))
        .is_some()
    {
        Err("run queue already exists for this core")
    } else {
        Ok(())
    }
}

pub fn get_run_queue(core: u8) -> Option<&'static RwLockPreempt<RunQueue<imp::TaskRef>>> {
    RUN_QUEUES.get(&core)
}

pub fn add_task_to_any_run_queue(task: task::TaskRef) -> Result<(), &'static str> {
    let task = imp::TaskRef::new(task);
    get_least_busy_run_queue()
        .ok_or("no run queues to add task to")?
        .write()
        .push_back(task);
    Ok(())
}

pub fn add_task_to_specific_run_queue(core: u8, task: task::TaskRef) -> Result<(), &'static str> {
    let task = imp::TaskRef::new(task);
    RUN_QUEUES
        .get(&core)
        .ok_or("run queue does not exist for core")?
        .write()
        .push_back(task);
    Ok(())
}

pub fn remove_task_from_all(task: &task::TaskRef) {
    for (_, run_queue) in RUN_QUEUES.iter() {
        run_queue.write().remove_task(task);
    }
}

fn get_least_busy_run_queue() -> Option<&'static RwLockPreempt<RunQueue<imp::TaskRef>>> {
    let mut min_rq: Option<(&'static RwLockPreempt<RunQueue<imp::TaskRef>>, usize)> = None;

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
    let run_queue = match RUN_QUEUES.get(&apic_id) {
        Some(rq) => rq.write(),
        _ => {
            error!(
                "BUG: schedule(): couldn't get run queue for core {}",
                apic_id
            );
            return false;
        }
    };

    let Some(imp::TaskRef { inner: next_task, .. }) = imp::select_next_task(run_queue) else {
        return false; // keep running the same current task
    };

    let (did_switch, recovered_preemption_guard) =
        task::task_switch(next_task, apic_id, preemption_guard);

    drop(recovered_preemption_guard);
    did_switch
}
