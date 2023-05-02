//! This crate picks the next task in round robin fashion.
//! Each time the task at the front of the queue is picked.
//! This task is then moved to the back of the queue. 

#![no_std]

extern crate alloc;

use alloc::boxed::Box;
use atomic_linked_list::atomic_map::AtomicMap;
use log::{error, trace};
use mutex_preemption::RwLockPreempt;
use runqueue_round_robin::RunqueueRoundRobin;
use scheduler_policy::{SchedulerPolicy, RunqueueError, RunqueueId, AllRunqueuesIterator, GenericSchedulerPolicy, ErasedGenericSchedulerPolicy, SimpleRunqueueTrait};
use task::TaskRef;


/// A basic round robin scheduler.
///
/// Currently, each CPU has its own runqueue;
/// there is no migration of tasks across CPUs/runqueues.
pub struct SchedulerRoundRobin {
    runqueues: AtomicMap<RunqueueId, RwLockPreempt<RunqueueRoundRobin>>,
}

impl SchedulerPolicy for SchedulerRoundRobin {
    fn init_runqueue(&self, rq_id: RunqueueId) -> Result<(), RunqueueError> {
        let new_rq = RwLockPreempt::new(RunqueueRoundRobin::new(rq_id));
        
        if self.runqueues.insert(rq_id, new_rq).is_none() {
            trace!("Created runqueue (round robin) for {:?}", rq_id);
            Ok(())
        } else {
            error!("BUG: SchedulerRoundRobin::init(): runqueue already exists with {:?}!", rq_id);
            Err(RunqueueError::RunqueueAlreadyExists)
        }
    }

    fn select_next_task(&self, rq_id: RunqueueId) -> Option<TaskRef> {
        let mut runqueue_locked = match self.get_runqueue(rq_id) {
            Some(rq) => rq.write(),
            None => {
                error!("BUG: select_next_task (round robin): couldn't get runqueue with {:?}", rq_id); 
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


    fn add_task(&self, task: TaskRef, rq_id: Option<RunqueueId>) -> Result<(), RunqueueError> {
        if let Some(id) = rq_id {
            self.add_task_to_specific_runqueue(id, task)
        } else {
            self.add_task_to_any_runqueue(task)
        }
    }

    fn remove_task(&self, task: &TaskRef) -> Result<(), RunqueueError> {
        for (_id, rq) in self.runqueues.iter() {
            rq.write().remove_task(task)?;
        }
        Ok(())
    }

    fn runqueue_iter(&self) -> scheduler_policy::AllRunqueuesIterator {
        AllRunqueuesIterator::from(self.runqueues.iter())
    }
}

impl SchedulerRoundRobin {
    /// Returns a new scheduler with no runqueues.
    pub fn new() -> Self {
        Self { runqueues: AtomicMap::new(), }
    }

    /// Returns the runqueue with the given ID.
    fn get_runqueue(&self, rq_id: RunqueueId) -> Option<&RwLockPreempt<RunqueueRoundRobin>> {
        self.runqueues.get(&rq_id)
    }

    /// Returns the "least busy" runqueue, currently based only on runqueue size.
    fn get_least_busy_runqueue(&self) -> Option<&RwLockPreempt<RunqueueRoundRobin>> {
        let mut min_rq: Option<(&RwLockPreempt<RunqueueRoundRobin>, usize)> = None;
        for (_, rq) in self.runqueues.iter() {
            let rq_size = rq.read().len();
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
    pub fn add_task_to_any_runqueue(&self, task: TaskRef) -> Result<(), RunqueueError> {
        let rq = self.get_least_busy_runqueue()
            .or_else(|| self.runqueues.iter().next().map(|r| r.1))
            .ok_or(RunqueueError::RunqueueNotFound)?;

        rq.write().add_task(task)
    }

    /// Adds the given task to the given runqueue.
    pub fn add_task_to_specific_runqueue(&self, rq_id: RunqueueId, task: TaskRef) -> Result<(), RunqueueError> {
        self.get_runqueue(rq_id)
            .ok_or(RunqueueError::RunqueueNotFound)?
            .write()
            .add_task(task)
    }
}


///////////////////////////////////////////////////////////////////////////////////////////
//////////////////  Attempt at an erased object-safe trait ////////////////////////////////
///////////////////////////////////////////////////////////////////////////////////////////

impl<'a> GenericSchedulerPolicy<'a> for SchedulerRoundRobin {
    // NOTE: i'd like to this to be a normal associated type:
    // 
    // type RunqueueType = RwLockPreempt<RunqueueRoundRobin>;

    type RunqueueType = dyn SimpleRunqueueTrait + 'a;

    fn with_runqueue(&'a self, rq_id: RunqueueId) -> Option<&'a Self::RunqueueType> {
        self.runqueues.get(&rq_id).map(|r| r as &Self::RunqueueType)
    }
}

// impl<'a> ErasedGenericSchedulerPolicy<'a> for SchedulerRoundRobin {
//     fn erased_with_runqueue(&self, rq_id: RunqueueId) -> Option<&dyn scheduler_policy::SimpleRunqueueTrait> {
//         self.with_runqueue(rq_id)
//     }
// }

pub fn do_work() {
    let id = RunqueueId::from(16u64);
    let sched_rr = SchedulerRoundRobin::new();
    sched_rr.init_runqueue(id).unwrap();

    let rq = sched_rr.with_runqueue(id).unwrap();
    log::debug!("rq {:?} has len {:?}", rq.id(), rq.len());

    let trait_obj: &dyn ErasedGenericSchedulerPolicy = &sched_rr; // as &dyn ErasedGenericSchedulerPolicy;
    // let trait_obj: Box<dyn ErasedGenericSchedulerPolicy> = Box::new(sched_rr); // as &dyn ErasedGenericSchedulerPolicy;
    
    let rq_ref = trait_obj.with_runqueue(id).unwrap();
    log::debug!("trait obj rq_ref {:?} has len {:?}", rq_ref.id(), rq_ref.len());

}