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
/// avoids having to lock the system wide list of schedulers.
#[cls::cpu_local]
static SCHEDULER: Option<&'static ConcurrentScheduler> = None;

type ConcurrentScheduler = Mutex<Box<dyn Scheduler>>;

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
            // TODO: Drain tasks from old scheduler and place into new scheduler.
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
    locked[least_busy_index.unwrap()].1.lock().push(task);
}

pub fn add_task_to(task: TaskRef, cpu_id: CpuId) {
    for (cpu, scheduler) in SCHEDULERS.lock().iter() {
        if *cpu == cpu_id {
            scheduler.lock().push(task);
            return;
        }
    }
}

pub fn remove_task(task: &TaskRef) -> bool {
    // TODO: T
    for (_, scheduler) in SCHEDULERS.lock().iter() {
        if scheduler.lock().remove(task) {
            return true;
        }
    }
    false
}

pub trait Scheduler: Send + Sync + 'static {
    fn next(&mut self) -> TaskRef;

    fn push(&mut self, task: TaskRef);

    fn busyness(&self) -> usize;

    fn remove(&mut self, task: &TaskRef) -> bool;

    fn as_priority_scheduler(&mut self) -> Option<&mut dyn PriorityScheduler>;
}

pub trait PriorityScheduler {
    fn set_priority(&mut self, task: &TaskRef);

    fn get_priority(&mut self, task: &TaskRef);

    fn inherit_priority(&mut self, task: &TaskRef);
}
