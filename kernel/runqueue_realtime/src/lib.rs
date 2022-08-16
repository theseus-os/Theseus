//! Runqueue structures for a realtime scheduler using rate monotonic scheduling.
//!
//! The `RunQueue` structure is essentially a list of `Task`s used for scheduling purposes.
//! Each `RealtimeTaskRef` element in the runqueue contains a `TaskRef` 
//! representing an underlying task and as well as a `period` value.
//! 
//! In rate monotonic scheduling, tasks are assigned fixed priorities in order of increasing periods.
//! Thus, the `period` value of a `RealtimeTaskRef` acts as a form of priority.
//! Each `RunQueue` consists of a `VecDeque` of `RealtimeTaskRef`s 
//! sorted in increasing order of their `period` values.
//! The sorting is maintained by inserting each `RealtimeTaskRef` at the proper index
//! according to its `period` value.
//!
//! Aperiodic tasks are assigned a `period` value of `None` and are placed at the back of the queue.
//! Since the scheduler iterates through the runqueue to select the first `Runnable` task,
//! lower-period tasks are "higher priority" and will be selected first, 
//! with aperiodic tasks being selected only when no periodic tasks are runnable.

#![no_std]

extern crate task;
extern crate irq_safety;
extern crate alloc;
#[macro_use] extern crate log;
extern crate atomic_linked_list;

use task::TaskRef;
use irq_safety::RwLockIrqSafe;
use alloc::collections::VecDeque;
use core::ops::{Deref, DerefMut};
use atomic_linked_list::atomic_map::AtomicMap;

/// A reference to a task with its period for realtime scheduling.
///
/// `RealtimeTaskRef` implements `Deref` and `DerefMut` traits, which dereferences to `TaskRef`.
#[derive(Debug, Clone)]
pub struct RealtimeTaskRef {
    /// `TaskRef` wrapped by `RealtimeTaskRef`
    taskref: TaskRef,
    /// `Some` if the task is periodic, `None` if it is aperiodic.
    period: Option<usize>,
    /// Number of context switches the task has undergone. Not used in scheduling algorithm
    context_switches: usize,
}

impl Deref for RealtimeTaskRef {
    type Target = TaskRef;
    fn deref(&self) -> &TaskRef {
        &self.taskref
    }
}

impl DerefMut for RealtimeTaskRef {
    fn deref_mut(&mut self) -> &mut TaskRef {
        &mut self.taskref
    }
}

impl RealtimeTaskRef {
    /// Creates a new `RealtimeTaskRef` that wraps the given `TaskRef`
    pub fn new(taskref: TaskRef, period: Option<usize>) -> RealtimeTaskRef {
        RealtimeTaskRef {
            taskref: taskref,
            period: period,
            context_switches: 0,
        }
    }

    /// Increment the number of times the task is picked
    pub fn increment_context_switches(&mut self) {
        self.context_switches = self.context_switches.saturating_add(1);
    }

    /// Checks whether the `RealtimeTaskRef` refers to a task that is periodic
    pub fn is_periodic(&self) -> bool {
        self.period.is_some()
    }

    /// Returns `true` if the period of this `RealtimeTaskRef` is shorter (less) than
    /// the period of the other `RealtimeTaskRef`.
    ///
    /// Returns `false` if this `RealtimeTaskRef` is aperiodic, i.e. if `period` is `None`.
    /// Returns `true` if this task is periodic and `other` is aperiodic.
    pub fn has_smaller_period(&self, other: &RealtimeTaskRef) -> bool {
        match self.period {
            Some(period_val) => if let Some(other_period_val) = other.period {
                period_val < other_period_val
            } else {
                true
            },
            None => false,
        }
    }
}

/// There is one runqueue per core, each core only accesses its own private runqueue
/// and allows the scheduler to select a task from that runqueue to schedule in
static RUNQUEUES: AtomicMap<u8, RwLockIrqSafe<RunQueue>> = AtomicMap::new();

/// A list of `Task`s and their associated realtime scheduler data that may be run on a given CPU core.
///
/// In rate monotonic scheduling, tasks are sorted in order of increasing periods.
/// Thus, the `period` value acts as a form of task "priority",
/// with higher priority (shorter period) tasks coming first.
#[derive(Debug)]
pub struct RunQueue {
    core: u8,
    queue: VecDeque<RealtimeTaskRef>,
}

impl Deref for RunQueue {
    type Target = VecDeque<RealtimeTaskRef>;
    fn deref(&self) -> &VecDeque<RealtimeTaskRef> {
        &self.queue
    }
}

impl DerefMut for RunQueue {
    fn deref_mut(&mut self) -> &mut VecDeque<RealtimeTaskRef> {
        &mut self.queue
    }
}


impl RunQueue {
    /// Moves the `RealtimeTaskRef` at the given `index` in this `RunQueue`
    /// to the appropriate location in this `RunQueue` based on its period.
    ///
    /// Returns a reference to the underlying `Task`.
    ///
    /// Thus, the `RealtimeTaskRef will be reinserted into the `RunQueue` so the `RunQueue` contains the
    /// `RealtimeTaskRef`s in order of increasing period. All aperiodic tasks will simply be reinserted at the end of the `RunQueue`
    /// in order to ensure no aperiodic tasks are selected until there are no periodic tasks ready for execution.
    /// Afterwards, the number of context switches is incremented by one.
    /// This function is used when the task is selected by the scheduler.
    pub fn update_and_reinsert(&mut self, index: usize) -> Option<TaskRef> {
        if let Some(mut realtime_taskref) = self.remove(index) {
            realtime_taskref.increment_context_switches();
            let taskref = realtime_taskref.taskref.clone();
            self.insert_realtime_taskref_at_proper_location(realtime_taskref);
            Some(taskref)
        }
        else {
            None
        }
    }

    /// Creates a new `RunQueue` for the given core, which is an `apic_id`
    pub fn init(which_core: u8) -> Result<(), &'static str> {
        #[cfg(not(loscd_eval))]
        trace!("Created runqueue (realtime) for core {}", which_core);
        let new_rq = RwLockIrqSafe::new(RunQueue {
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

        rq.write().add_task(task, None)
    }

    /// Convenience method that adds the given `Task` reference to given core's runqueue.
    pub fn add_task_to_specific_runqueue(which_core: u8, task: TaskRef) -> Result<(), &'static str> {
        RunQueue::get_runqueue(which_core)
            .ok_or("Couldn't get RunQueue for the given core")?
            .write()
            .add_task(task, None)
    }

    /// Inserts a `RealtimeTaskRef` at its proper position in the queue.
    ///
    /// Under the RMS scheduling algorithm, tasks should be sorted in increasing value 
    /// of their periods, with aperiodic tasks being placed at the end.
    fn insert_realtime_taskref_at_proper_location(&mut self, taskref: RealtimeTaskRef) {
        match taskref.period {
            None => self.push_back(taskref),
            Some(_) => {
                if self.is_empty() {
                    self.push_back(taskref);
                } else {
                    let mut index_to_insert: Option<usize> = None;
                    for (index, inserted_taskref) in self.iter().enumerate() {
                        if taskref.has_smaller_period(inserted_taskref) {
                            index_to_insert = Some(index);
                            break;
                        }
                    }

                    if let Some(index) = index_to_insert {
                        self.insert(index, taskref);
                    } else {
                        self.push_back(taskref);
                    }
                }
            }
        }
    }

    /// Adds a `TaskRef` to this runqueue with the given periodicity value
    fn add_task(&mut self, task: TaskRef, period: Option<usize>) -> Result<(), &'static str> {
        debug!("Adding task to runqueue_realtime {}, {:?}", self.core, task);
        let realtime_taskref = RealtimeTaskRef::new(task, period);
        self.insert_realtime_taskref_at_proper_location(realtime_taskref);

        Ok(())
    }
    
    /// The internal function that actually removes the task from the runqueue.
    fn remove_internal(&mut self, task: &TaskRef) -> Result<(), &'static str> {
        debug!("Removing task from runqueue_realtime {}, {:?}", self.core, task);
        self.retain(|x| &x.taskref != task);

        Ok(())
    }

    /// Removes a `TaskRef` from this RunQueue.
    pub fn remove_task(&mut self, task: &TaskRef) -> Result<(), &'static str> {
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

    /// The internal function that sets the periodicity of a given `Task` in a single `RunQueue`
    /// then reinserts the `RealtimeTaskRef` at the proper location
    fn set_periodicity_internal(
        &mut self, 
        task: &TaskRef, 
        period: usize
    ) -> Result<(), &'static str> {
        match self.iter().position(|rt| rt.taskref == *task ) {
            Some(i) => {
                if let Some(mut realtime_taskref) = self.remove(i) {
                    realtime_taskref.period = Some(period);
                    self.insert_realtime_taskref_at_proper_location(realtime_taskref);
                }
            },
            None => {},
        };
        Ok(())
    }
}

/// Set the periodicity of a given `Task` in all the `RunQueue` structures
pub fn set_periodicity(task: &TaskRef, period: usize) -> Result<(), &'static str> {
    for (_core, rq) in RUNQUEUES.iter() {
        rq.write().set_periodicity_internal(task, period)?;
    }
    Ok(())
}
