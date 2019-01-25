//! This crate contains the `RunQueue` structure, for round robin scheduler. 
//! `RunQueue` structure is essentially a list of Tasks
//! that is used for scheduling purposes.
//! 

#![no_std]
#![feature(alloc)]

extern crate alloc;
#[macro_use] extern crate lazy_static;
#[macro_use] extern crate log;
extern crate irq_safety;
extern crate atomic_linked_list;
extern crate task;

#[cfg(single_simd_task_optimization)]
extern crate single_simd_task_optimization;

use alloc::collections::VecDeque;
use irq_safety::{RwLockIrqSafe, MutexIrqSafeGuardRef};
use atomic_linked_list::atomic_map::AtomicMap;
use task::{TaskRef, Task};

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
#[derive(Debug, Clone)]
pub struct RoundRobinTaskRef{
    /// `TaskRef` wrapped by `RoundRobinTaskRef`
    taskref: TaskRef,

    /// Number of context switches the task has undergone. Not used in scheduling algorithm
    context_switches: u32,
}

impl RoundRobinTaskRef {
    /// Creates a new `RoundRobinTaskRef` that wraps the given `TaskRef`.
    pub fn new(taskref: TaskRef) -> RoundRobinTaskRef {
        let round_robin_taskref = RoundRobinTaskRef {
            taskref: taskref,
            context_switches: 0,
        };
        round_robin_taskref
    }

    /// Get a pointer for the underlying TaskRef
    pub fn get_task_ref(round_robin_taskref: Option<RoundRobinTaskRef>) -> Option<TaskRef> {
        round_robin_taskref.map(|m| m.taskref)
    }

    /// Obtains the lock on the underlying `Task` in a read-only, blocking fashion.
    pub fn lock(&self) -> MutexIrqSafeGuardRef<Task> {
       self.taskref.lock()
    }

    /// Increment the number of times the task is picked
    pub fn increment_context_switches(&mut self) -> (){
        self.context_switches = self.context_switches.saturating_add(1);
    }
}

lazy_static! {
    /// There is one runqueue per core, each core only accesses its own private runqueue
    /// and allows the scheduler to select a task from that runqueue to schedule in.
    static ref RUNQUEUES: AtomicMap<u8, RwLockIrqSafe<RunQueue>> = AtomicMap::new();
}

/// A list of references to `Task`s (`RoundRobinTaskRef`s). 
/// This is used to store the `Task`s (and associated scheduler related data) 
/// that are runnable on a given core.
/// A queue is used for the round robin scheduler.
#[derive(Debug)]
pub struct RunQueue {
    core: u8,
    queue: VecDeque<RoundRobinTaskRef>,
}

/// `RunQueue` functions are listed here. This is a superset of functions listed in
/// `RunQueue` crate.
impl RunQueue {
    
    /// Moves the `TaskRef` at the given index into this `RunQueue` to the end (back) of this `RunQueue`,
    /// and returns a cloned reference to that `TaskRef`.
    pub fn move_to_end(&mut self, index: usize) -> Option<TaskRef> {
        self.queue.remove(index).map(|taskref| {
            self.queue.push_back(taskref.clone());
            taskref
        }).map(|m| m.taskref)
    }

    /// Returns an iterator over all `TaskRef`s in this `RunQueue`.
    pub fn iter(&self) -> alloc::collections::vec_deque::Iter<RoundRobinTaskRef> {
        self.queue.iter()
    }
   
    /// Creates a new `RunQueue` for the given core, which is an `apic_id`.
    pub fn init(which_core: u8) -> Result<(), &'static str> {
        trace!("Created runqueue for core {}", which_core);
        let new_rq = RwLockIrqSafe::new(RunQueue {
            core: which_core,
            queue: VecDeque::new(),
        });

        #[cfg(runqueue_state_spill_evaluation)] 
        {
            task::RUNQUEUE_REMOVAL_FUNCTION.call_once(|| RunQueue::remove_task_from_within_task);
        }

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
    pub fn get_runqueue(which_core: u8) -> Option<&'static RwLockIrqSafe<RunQueue>> {
        RUNQUEUES.get(&which_core)
    }


    /// Returns the "least busy" core, which is currently very simple, based on runqueue size.
    pub fn get_least_busy_core() -> Option<u8> {
        Self::get_least_busy_runqueue().map(|rq| rq.read().core)
    }


    /// Returns the `RunQueue` for the "least busy" core.
    /// See [`get_least_busy_core()`](#method.get_least_busy_core)
    fn get_least_busy_runqueue() -> Option<&'static RwLockIrqSafe<RunQueue>> {
        let mut min_rq: Option<(&'static RwLockIrqSafe<RunQueue>, usize)> = None;

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
            .add_task(task.clone())
    }

    /// Adds a `TaskRef` to this RunQueue.
    fn add_task(&mut self, task: TaskRef) -> Result<(), &'static str> {        
        #[cfg(single_simd_task_optimization)]
        let is_simd = task.lock().simd;
        
        #[cfg(runqueue_state_spill_evaluation)]
        {
            task.lock_mut().on_runqueue = Some(self.core);
        }

        debug!("Adding task to runqueue {}, {:?}", self.core, task);
        let round_robin_taskref = RoundRobinTaskRef::new(task);
        self.queue.push_back(round_robin_taskref);
        
        #[cfg(single_simd_task_optimization)]
        {   
            warn!("USING SINGLE_SIMD_TASK_OPTIMIZATION VERSION OF RUNQUEUE::ADD_TASK");
            // notify simd_personality crate about runqueue change, but only for SIMD tasks
            if is_simd {
                single_simd_task_optimization::simd_tasks_added_to_core(self.iter(), self.core);
            }
        }

        Ok(())
    }


    /// Retrieves the `TaskRef` in this `RunQueue` at the specified `index`.
    /// Index 0 is the front of the RunQueue.
    pub fn get(&self, index: usize) -> Option<&TaskRef> {
        self.queue.get(index).map(|m| &m.taskref)
    }


    /// The internal function that actually removes the task from the runqueue.
    fn remove_internal(&mut self, task: &TaskRef) -> Result<(), &'static str> {
        // debug!("Removing task from runqueue {}, {:?}", self.core, task);
        self.queue.retain(|x| &x.taskref != task);

        #[cfg(single_simd_task_optimization)]
        {   
            let is_simd = { task.lock().simd };
            warn!("USING SINGLE_SIMD_TASK_OPTIMIZATION VERSION OF RUNQUEUE::REMOVE_TASK");
            // notify simd_personality crate about runqueue change, but only for SIMD tasks
            if is_simd {
                single_simd_task_optimization::simd_tasks_removed_from_core(self.iter(), self.core);
            }
        }

        Ok(())
    }


    /// Removes a `TaskRef` from this RunQueue.
    pub fn remove_task(&mut self, task: &TaskRef) -> Result<(), &'static str> {
        #[cfg(runqueue_state_spill_evaluation)]
        {
            // For the runqueue state spill evaluation, we disable this method because we 
            // only want to allow removing a task from a runqueue from within the TaskRef::internal_exit() method.
            // trace!("skipping remove_task() on core {}, task {:?}", self.core, task);
            return Ok(());
        }

        self.remove_internal(task)
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


    #[cfg(runqueue_state_spill_evaluation)]
    /// Removes a `TaskRef` from the RunQueue(s) on the given `core`.
    /// Note: This method is only used by the state spillful runqueue implementation.
    pub fn remove_task_from_within_task(task: &TaskRef, core: u8) -> Result<(), &'static str> {
        // warn!("remove_task_from_within_task(): core {}, task: {:?}", core, task);
        task.lock_mut().on_runqueue = None;
        RUNQUEUES.get(&core)
            .ok_or("Couldn't get runqueue for specified core")
            .and_then(|rq| {
                // Instead of calling `remove_task`, we directly call `remove_internal`
                // because we want to actually remove the task from the runqueue,
                // as calling `remove_task` would do nothing due to it skipping the actual removal
                // when the `runqueue_state_spill_evaluation` cfg is enabled.
                rq.write().remove_internal(task)
            })
    }
}
