//! This crate contains the `RunQueue` structure, for priority scheduler. 
//! `RunQueue` structure is essentially a list of Tasks
//! that it used for scheduling purposes.
//! 

#![no_std]

extern crate alloc;
#[macro_use] extern crate log;
extern crate irq_safety;
extern crate atomic_linked_list;
extern crate task;

#[cfg(single_simd_task_optimization)]
extern crate single_simd_task_optimization;

use alloc::collections::VecDeque;
use irq_safety::RwLockIrqSafe;
use atomic_linked_list::atomic_map::AtomicMap;
use task::TaskRef;
use core::ops::{Deref, DerefMut};

pub const MAX_PRIORITY: u8 = 40;
pub const DEFAULT_PRIORITY: u8 = 20;
pub const INITIAL_TOKENS: usize = 10;

/// A cloneable reference to a `Taskref` that exposes more methods
/// related to task scheduling.
/// 
/// The `PriorityTaskRef` type is necessary since differnt scheduling algorithms 
/// require different data associated with the task to be stored alongside.
/// This makes storing them alongside the task prohibitive.
/// context_switches is not used in scheduling algorithm
/// `PriorityTaskRef` implements `Deref` and `DerefMut` traits, which dereferences to `TaskRef`. 
#[derive(Debug, Clone)]
pub struct PriorityTaskRef{
    /// `TaskRef` wrapped by `PriorityTaskRef`
    taskref: TaskRef,

    /// Priority assigned for the task. Max priority = 40, Min priority = 0.
    pub priority: u8,

    /// Remaining tokens in this epoch. A task will be scheduled in an epoch until tokens run out
    pub tokens_remaining: usize,

    /// Number of context switches the task has undergone. Not used in scheduling algorithm
    context_switches: usize,
}

impl Deref for PriorityTaskRef {
    type Target = TaskRef;
    fn deref(&self) -> &TaskRef {
        &self.taskref
    }
}

impl DerefMut for PriorityTaskRef {
    fn deref_mut(&mut self) -> &mut TaskRef {
        &mut self.taskref
    }
}

impl PriorityTaskRef {
    /// Creates a new `PriorityTaskRef` that wraps the given `TaskRef`.
    /// We just give an initial number of tokens to run the task till 
    /// next scheduling epoch
    pub fn new(taskref: TaskRef) -> PriorityTaskRef {
        let priority_taskref = PriorityTaskRef {
            taskref: taskref,
            priority: DEFAULT_PRIORITY,
            tokens_remaining: INITIAL_TOKENS,
            context_switches: 0,
        };
        priority_taskref
    }

    /// Increment the number of times the task is picked
    pub fn increment_context_switches(&mut self) -> (){
        self.context_switches = self.context_switches.saturating_add(1);
    }
}


/// There is one runqueue per core, each core only accesses its own private runqueue
/// and allows the scheduler to select a task from that runqueue to schedule in.
static RUNQUEUES: AtomicMap<u8, RwLockIrqSafe<RunQueue>> = AtomicMap::new();

/// A list of references to `Task`s (`PriorityTaskRef`s) 
/// that is used to store the `Task`s (and associated scheduler related data) 
/// that are runnable on a given core.
/// A queue is used for the token based prioirty scheduler.
/// `Runqueue` implements `Deref` and `DerefMut` traits, which dereferences to `VecDeque`.
#[derive(Debug)]
pub struct RunQueue {
    core: u8,
    queue: VecDeque<PriorityTaskRef>,
}

impl Deref for RunQueue {
    type Target = VecDeque<PriorityTaskRef>;
    fn deref(&self) -> &VecDeque<PriorityTaskRef> {
        &self.queue
    }
}

impl DerefMut for RunQueue {
    fn deref_mut(&mut self) -> &mut VecDeque<PriorityTaskRef> {
        &mut self.queue
    }
}

impl RunQueue {

    /// Moves the `TaskRef` at the given index in this `RunQueue` to the end (back) of this `RunQueue`,
    /// and returns a cloned reference to that `TaskRef`. The number of tokens is reduced by one and number of context
    /// switches is increased by one. This function is used when the task is selected by the scheduler
    pub fn update_and_move_to_end(&mut self, index: usize, tokens : usize) -> Option<TaskRef> {
        if let Some(mut priority_task_ref) = self.remove(index) {
            priority_task_ref.tokens_remaining = tokens;
            priority_task_ref.increment_context_switches();
            let taskref = priority_task_ref.taskref.clone();
            self.push_back(priority_task_ref);
            Some(taskref)
        } 
        else {
            None 
        }
    }

    /// Creates a new `RunQueue` for the given core, which is an `apic_id`
    pub fn init(which_core: u8) -> Result<(), &'static str> {
        #[cfg(not(loscd_eval))]
        trace!("Created runqueue (priority) for core {}", which_core);
        let new_rq = RwLockIrqSafe::new(RunQueue {
            core: which_core,
            queue: VecDeque::new(),
        });

        #[cfg(runqueue_spillful)] 
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

    /// Returns `RunQueue` for the given core, which is an `apic_id`.
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
            .add_task(task)
    }

    /// Adds a `TaskRef` to this RunQueue.
    fn add_task(&mut self, task: TaskRef) -> Result<(), &'static str> {        
        #[cfg(runqueue_spillful)] {
            task.set_on_runqueue(Some(self.core));
        }

        #[cfg(not(loscd_eval))]
        debug!("Adding task to runqueue_priority {}, {:?}", self.core, task);
        let priority_task_ref = PriorityTaskRef::new(task);
        self.push_back(priority_task_ref);
        
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

    /// The internal function that actually removes the task from the runqueue.
    fn remove_internal(&mut self, task: &TaskRef) -> Result<(), &'static str> {
        debug!("Removing task from runqueue_priority {}, {:?}", self.core, task);
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


    /// Removes a `TaskRef` from this RunQueue.
    pub fn remove_task(&mut self, _task: &TaskRef) -> Result<(), &'static str> {
        #[cfg(runqueue_spillful)] {
            // For the runqueue state spill evaluation, we disable this method because we 
            // only want to allow removing a task from a runqueue from within the TaskRef::internal_exit() method.
            // trace!("skipping remove_task() on core {}, task {:?}", self.core, _task);
            return Ok(());
        }
        #[cfg(not(runqueue_spillful))] {
            self.remove_internal(_task)
        }
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


    #[cfg(runqueue_spillful)]
    /// Removes a `TaskRef` from the RunQueue(s) on the given `core`.
    /// Note: This method is only used by the state spillful runqueue implementation.
    pub fn remove_task_from_within_task(task: &TaskRef, core: u8) -> Result<(), &'static str> {
        // warn!("remove_task_from_within_task(): core {}, task: {:?}", core, task);
        task.set_on_runqueue(None);
        RUNQUEUES.get(&core)
            .ok_or("Couldn't get runqueue for specified core")
            .and_then(|rq| {
                // Instead of calling `remove_task`, we directly call `remove_internal`
                // because we want to actually remove the task from the runqueue,
                // as calling `remove_task` would do nothing due to it skipping the actual removal
                // when the `runqueue_spillful` cfg is enabled.
                rq.write().remove_internal(task)
            })
    }

    /// The internal function that sets the priority of a given `Task` in a single `RunQueue`
    fn set_priority_internal(&mut self, task: &TaskRef, priority: u8) -> Result<(), &'static str> {
        // debug!("called_assign_priority_internal called per core");
        for x in self.iter_mut() {
            if &x.taskref == task{
                debug!("changed priority from {}  to {} ", x.priority, priority);
                x.priority = priority;
            }
        }
        Ok(())
    } 

    /// Sets the priority of the given `Task` in all the `RunQueue` structures 
    pub fn set_priority(task: &TaskRef, priority: u8) -> Result<(), &'static str> {
        // debug!("assign priority wrapper. called once per call");
        for (_core, rq) in RUNQUEUES.iter() {
            rq.write().set_priority_internal(task, priority)?;
        }
        Ok(())
    }

    /// The internal function that outputs the priority of a given task.
    /// The priority of the first task that matches is shown.
    fn get_priority_internal(&self, task: &TaskRef) -> Option<u8> {
        // debug!("called_assign_priority_internal called per core");
        for x in self.iter() {
            if &x.taskref == task {
                return Some(x.priority);
            }
        }
        None
    }

    /// Output the priority of the given task.
    /// Outputs None if the task is not found in any of the runqueues.
    pub fn get_priority(task: &TaskRef) -> Option<u8> {
        // debug!("assign priority wrapper. called once per call");
        for (_core, rq) in RUNQUEUES.iter() {
            let return_priority = rq.read().get_priority_internal(task);
            match return_priority {
                //If a matching task is found the iteration terminates
                Some(x) => return Some(x),
                None => continue,
            }
        }
        return None;
    }

}
