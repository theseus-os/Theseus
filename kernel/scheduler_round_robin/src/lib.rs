//! This crate picks the next task in round robin fashion.
//! Each time the task at the front of the queue is picked.
//! This task is then moved to the back of the queue. 

#![no_std]

extern crate alloc;

use atomic_linked_list::atomic_map::AtomicMap;
use mutex_preemption::RwLockPreempt;
use task::TaskRef;
use runqueue_round_robin::RunqueueRoundRobin;


/// Each CPU has its own private runqueue instance;
/// there is currently no migration of tasks across CPUs.
///
/// TODO: move this into per-CPU data regions.
static RUNQUEUES: AtomicMap<CpuId, RwLockPreempt<RunqueueRoundRobin>> = AtomicMap::new();


pub struct SchedulerRoundRobin;

impl SchedulerRoundRobin {
    /// Creates a new runqueue for the given `cpu`.
    pub fn init(cpu: CpuId) -> Result<(), &'static str> {
        let new_rq = RwLockPreempt::new(RunqueueRoundRobin::new(cpu));
        
        if RUNQUEUES.insert(cpu, new_rq).is_none() {
            trace!("Created runqueue (round robin) for CPU {}", cpu);
            Ok(())
        } else {
            error!("BUG: SchedulerRoundRobin::init(): runqueue already exists for CPU {}!", cpu);
            Err("BUG: SchedulerRoundRobin::init(): runqueue already exists for CPU {}!")
        }
    }

    /// This defines the round robin scheduler policy.
    /// Returns None if there is no schedule-able task
    pub fn select_next_task(cpu: CpuId) -> Option<TaskRef> {
        let mut runqueue_locked = match Self::get_runqueue(cpu) {
            Some(rq) => rq.write(),
            None => {
                error!("BUG: select_next_task (round robin): couldn't get runqueue for CPU {}", cpu); 
                return None;
            }
        };
        
        let mut idle_task_index: Option<usize> = None;
        let mut chosen_task_index: Option<usize> = None;

        for (i, t) in runqueue_locked.iter().enumerate() {
            // we skip the idle task, and only choose it if no other tasks are runnable
            if t.is_an_idle_task {
                idle_task_index = Some(i);
                continue;
            }

            // must be runnable
            if !t.is_runnable() {
                continue;
            }
                
            // found a runnable task!
            chosen_task_index = Some(i);
            // debug!("select_next_task(): AP {} chose Task {:?}", apic_id, &*t);
            break;
        }

        // idle task is a backup iff no other task has been chosen
        chosen_task_index
            .or(idle_task_index)
            .and_then(|index| runqueue_locked.move_to_end(index))
    }


    /// Removes the given task from all runqueues.
    ///
    /// This is a brute force approach that iterates over all runqueues. 
    pub fn remove_task_from_all_runqueues(task: &TaskRef) -> Result<(), &'static str> {
        for (_cpu, rq) in RUNQUEUES.iter() {
            rq.write().remove_task(task)?;
        }
        Ok(())
    }

    /// Returns the runqueue for the given CPU.
    pub fn get_runqueue(cpu: CpuId) -> Option<&'static RwLockPreempt<RunqueueRoundRobin>> {
        RUNQUEUES.get(&cpu)
    }

    /// Returns the "least busy" runqueue, currently based only on runqueue size.
    pub fn get_least_busy_runqueue() -> Option<&'static RwLockPreempt<RunqueueRoundRobin>> {
        let mut min_rq = None;
        for (_, rq) in RUNQUEUES.iter() {
            let rq_size = rq.read().queue.len();
            if let Some(min) = min_rq {
                if rq_size < min.1 {
                    min_rq = Some((rq, rq_size));
                }
            }
            else {
                min_rq = Some((rq, rq_size));
            }
        }

        min_rq.map(|m| m.0)
    }

    /// Adds the given task to the "least busy" runqueue.
    pub fn add_task_to_any_runqueue(task: impl Into<RoundRobinTaskRef>) -> Result<(), &'static str> {
        let rq = Self::get_least_busy_runqueue()
            .or_else(|| RUNQUEUES.iter().next().map(|r| r.1))
            .ok_or("couldn't find any runqueues to add the task to!")?;

        rq.write().add_task(task)
    }

    /// Adds the given task to the given CPU's runqueue.
    pub fn add_task_to_specific_runqueue(cpu: CpuId, task: impl Into<RoundRobinTaskRef>) -> Result<(), &'static str> {
        Self::get_runqueue(cpu)
            .ok_or("Couldn't get runqueue for the given CPU")?
            .write()
            .add_task(task)
    }
}
