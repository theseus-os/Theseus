//! This crate contains the `RunQueue` structure, for round robin scheduler. 
//! `RunQueue` structure is essentially a list of Tasks
//! that is used for scheduling purposes.
//! 

#![no_std]

extern crate alloc;
#[macro_use] extern crate log;
extern crate mutex_preemption;
extern crate atomic_linked_list;
extern crate task;

#[cfg(single_simd_task_optimization)]
extern crate single_simd_task_optimization;

use alloc::collections::VecDeque;
use mutex_preemption::RwLockPreempt;
use atomic_linked_list::atomic_map::AtomicMap;
use task::TaskRef;
use core::ops::{Deref, DerefMut};

/// A cloneable reference to a `Taskref` that exposes more methods
/// related to task scheduling
/// 
/// The `RoundRobinTaskRef` type is necessary since differnt scheduling algorithms 
/// require different data associated with the task to be stored alongside.
/// This makes storing them alongside the task prohibitive.
/// Since round robin is the most primitive scheduling policy 
/// no additional scheduling information is needed.
/// context_switches indicate the number of context switches
/// the task has undergone.
/// context_switches is not used in scheduling algorithm.
/// `RoundRobinTaskRef` implements `Deref` and `DerefMut` traits, which dereferences to `TaskRef`.  
#[derive(Debug, Clone)]
pub struct RoundRobinTaskRef{
    /// `TaskRef` wrapped by `RoundRobinTaskRef`
    taskref: TaskRef,

    /// Number of context switches the task has undergone. Not used in scheduling algorithm
    context_switches: usize,
}

// impl Drop for RoundRobinTaskRef {
//     fn drop(&mut self) {
//         warn!("DROPPING RoundRobinTaskRef with taskref {:?}", self.taskref);
//     }
// }

impl Deref for RoundRobinTaskRef {
    type Target = TaskRef;
    fn deref(&self) -> &TaskRef {
        &self.taskref
    }
}

impl DerefMut for RoundRobinTaskRef {
    fn deref_mut(&mut self) -> &mut TaskRef {
        &mut self.taskref
    }
}

impl RoundRobinTaskRef {
    /// Creates a new `RoundRobinTaskRef` that wraps the given `TaskRef`.
    pub fn new(taskref: TaskRef) -> RoundRobinTaskRef {
        RoundRobinTaskRef {
            taskref,
            context_switches: 0,
        }
    }

    /// Increment the number of times the task is picked
    pub fn increment_context_switches(&mut self) {
        self.context_switches = self.context_switches.saturating_add(1);
    }
}

/// There is one runqueue per core, each core only accesses its own private runqueue
/// and allows the scheduler to select a task from that runqueue to schedule in.
pub static RUNQUEUES: AtomicMap<u8, RwLockPreempt<RunQueue>> = AtomicMap::new();

/// A list of references to `Task`s (`RoundRobinTaskRef`s). 
/// This is used to store the `Task`s (and associated scheduler related data) 
/// that are runnable on a given core.
/// A queue is used for the round robin scheduler.
/// `Runqueue` implements `Deref` and `DerefMut` traits, which dereferences to `VecDeque`.
#[derive(Debug)]
pub struct RunQueue {
    core: u8,
    queue: VecDeque<RoundRobinTaskRef>,
}
// impl Drop for RunQueue {
//     fn drop(&mut self) {
//         warn!("DROPPING Round Robing Runqueue for core {}", self.core);
//     }
// }

impl Deref for RunQueue {
    type Target = VecDeque<RoundRobinTaskRef>;
    fn deref(&self) -> &VecDeque<RoundRobinTaskRef> {
        &self.queue
    }
}

impl DerefMut for RunQueue {
    fn deref_mut(&mut self) -> &mut VecDeque<RoundRobinTaskRef> {
        &mut self.queue
    }
}

impl RunQueue {
    
    /// Moves the `TaskRef` at the given index into this `RunQueue` to the end (back) of this `RunQueue`,
    /// and returns a cloned reference to that `TaskRef`.
    pub fn move_to_end(&mut self, index: usize) -> Option<TaskRef> {
        self.swap_remove_front(index).map(|rr_taskref| {
            let taskref = rr_taskref.taskref.clone();
            self.push_back(rr_taskref);
            taskref
        })
    }

    /// Creates a new `RunQueue` for the given core, which is an `apic_id`.
    pub fn init(which_core: u8) -> Result<(), &'static str> {
        trace!("Created runqueue (round robin) for core {}", which_core);
        let new_rq = RwLockPreempt::new(RunQueue {
            core: which_core,
            queue: VecDeque::new(),
        });

        if RUNQUEUES.insert(which_core, new_rq).is_some() {
            error!("BUG: RunQueue::init(): runqueue already exists for core {}!", which_core);
            Err("runqueue already exists for this core")
        }
        else {
            // there shouldn't already be a RunQueue for this core
            Ok(())
        }
    }

    /// Returns the `RunQueue` for the given core, which is an `apic_id`.
    pub fn get_runqueue(which_core: u8) -> Option<&'static RwLockPreempt<RunQueue>> {
        RUNQUEUES.get(&which_core)
    }


    /// Returns the "least busy" core, which is currently very simple, based on runqueue size.
    pub fn get_least_busy_core() -> Option<u8> {
        Self::get_least_busy_runqueue().map(|rq| rq.read().core)
    }


    /// Returns the `RunQueue` for the "least busy" core.
    /// See [`get_least_busy_core()`](#method.get_least_busy_core)
    fn get_least_busy_runqueue() -> Option<&'static RwLockPreempt<RunQueue>> {
        let mut min_rq: Option<(&'static RwLockPreempt<RunQueue>, usize)> = None;

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

    /// Chooses the "least busy" core's runqueue (based on simple runqueue-size-based load balancing)
    /// and adds the given `Task` reference to that core's runqueue.
    pub fn add_task_to_any_runqueue(task: TaskRef) -> Result<(), &'static str> {
        let rq = RunQueue::get_least_busy_runqueue()
            .or_else(|| RUNQUEUES.iter().next().map(|r| r.1))
            .ok_or("couldn't find any runqueues to add the task to!")?;

        rq.write().add_task(task)
    }

    /// Convenience method that adds the given `Task` reference to given core's runqueue.
    pub fn add_task_to_specific_runqueue(which_core: u8, task: TaskRef) -> Result<(), &'static str> {
        RunQueue::get_runqueue(which_core)
            .ok_or("Couldn't get RunQueue for the given core")?
            .write()
            .add_task(task)
    }

    /// Adds a `TaskRef` to this RunQueue.
    fn add_task(&mut self, task: TaskRef) -> Result<(), &'static str> {        
        #[cfg(not(any(rq_eval, downtime_eval)))]
        debug!("Adding task to runqueue_round_robin {}, {:?}", self.core, task);

        let round_robin_taskref = RoundRobinTaskRef::new(task);
        self.push_back(round_robin_taskref);
        
        #[cfg(single_simd_task_optimization)]
        {   
            warn!("USING SINGLE_SIMD_TASK_OPTIMIZATION VERSION OF RUNQUEUE::ADD_TASK");
            // notify simd_personality crate about runqueue change, but only for SIMD tasks
            if task.simd {
                single_simd_task_optimization::simd_tasks_added_to_core(self.iter(), self.core);
            }
        }

        Ok(())
    }

    /// Removes a `TaskRef` from this RunQueue.
    pub fn remove_task(&mut self, task: &TaskRef) -> Result<(), &'static str> {
        #[cfg(not(any(rq_eval, downtime_eval)))]
        debug!("Removing task from runqueue_round_robin {}, {:?}", self.core, task);
        self.retain(|x| &x.taskref != task);

        #[cfg(single_simd_task_optimization)] {   
            warn!("USING SINGLE_SIMD_TASK_OPTIMIZATION VERSION OF RUNQUEUE::REMOVE_TASK");
            // notify simd_personality crate about runqueue change, but only for SIMD tasks
            if task.simd {
                single_simd_task_optimization::simd_tasks_removed_from_core(self.iter(), self.core);
            }
        }

        Ok(())
    }


    /// Removes a `TaskRef` from all `RunQueue`s that exist on the entire system.
    /// 
    /// This is a brute force approach that iterates over all runqueues. 
    pub fn remove_task_from_all(task: &TaskRef) -> Result<(), &'static str> {
        for (_core, rq) in RUNQUEUES.iter() {
            rq.write().remove_task(task)?;
        }
        Ok(())
    }
}
