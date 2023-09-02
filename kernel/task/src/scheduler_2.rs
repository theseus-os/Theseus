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

pub fn set_policy<T>(scheduler: T)
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
        let current_cpu = cpu::current_cpu();

        if let Some(old_scheduler) = current_scheduler {
            // TODO: Drain tasks from old scheduler and place into new scheduler.
            error!("replacing existing scheduler: this is not currently supported");

            let mut old_scheduler_index = None;
            for (i, (cpu, scheduler)) in locked.iter().enumerate() {
                if *cpu == current_cpu {
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

        locked.push((current_cpu, scheduler_ref));
        *current_scheduler = Some(scheduler_ref);
    });
}

fn next_task() -> Option<TaskRef> {
    SCHEDULER.update(|scheduler| scheduler.unwrap().lock().as_mut().next())
}

pub trait Scheduler: Send + Sync + 'static {
    fn next(&mut self) -> Option<TaskRef>;

    fn as_priority_scheduler(&mut self) -> Option<&mut dyn PriorityScheduler>;
}

pub trait PriorityScheduler {
    fn set_priority(&mut self, task: &TaskRef);

    fn get_priority(&mut self, task: &TaskRef);

    fn inherit_priority(&mut self, task: &TaskRef);
}
