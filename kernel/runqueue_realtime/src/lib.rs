//! This crate contains the `RunQueue` structure, for a realtime scheduler using rate monotonic scheduling.
//! `RunQueue` structure is essentially a list of `Task`s
//! that is used for scheduling purposes.
//!

#![no_std]

extern crate task;
extern crate irq_safety;
extern crate alloc;
#[macro_use] extern crate log;
#[macro_use] extern crate lazy_static;
extern crate atomic_linked_list;

use task::TaskRef;
use irq_safety::RwLockIrqSafe;
use alloc::collections::VecDeque;
use core::ops::{Deref, DerefMut};
use atomic_linked_list::atomic_map::AtomicMap;

/// A cloneable reference to a `Taskref` that exposes more methods
/// related to task scheduling
/// 
/// Each `RealtimeTaskRef` contains additional information on top a `TaskRef` object
/// In the case of realtime scheduling, we will be using the RMS algorithm,
/// thus, it is necessary to know whether a task is periodic.
/// If so, the field `period` will contain the period as an integer wrapped in a `Some` object.
/// If the task is aperiodic, `period` will contain the value `None`
/// `RealtimeTaskRef` implements `Deref` and `DerefMut` traits, which dereferences to `TaskRef`
#[derive(Debug, Clone)]
pub struct RealtimeTaskRef {
	/// `TaskRef` wrapped by `RealtimeTaskRef`
	taskref: TaskRef,

	/// If the task is periodic, this value will be the period in ticks wrapped in `Some`
	/// If the task is aperiodic, this value will be `None`
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
		match self.period {
			Some(_) => true,
			None => false,
		}
	}

	/// Checks whether the period of this `RealtimeTaskRef` is shorter than the period of another `RealtimeTaskRef`
	/// If the `RealtimeTaskRef` is aperiodic, i.e. if `period` is `None`, we will always return false
	/// Additionally, a periodic task will always return `true` if `other_taskref` is aperiodic
	pub fn has_smaller_period(&self, other_taskref: &RealtimeTaskRef) -> bool{
		match self.period {
			Some(period_val) => if let Some(other_period_val) = other_taskref.period {
				period_val < other_period_val
			} else {
				true
			},
			None => false,
		}
	}
}

lazy_static! {
	/// There is one runqueue per core, each core only accesses its own private runqueue
	/// and allows the scheduler to select a task from that runqueue to schedule in
	static ref RUNQUEUES: AtomicMap<u8, RwLockIrqSafe<RunQueue>> = AtomicMap::new();
}

/// A list of references to `Task`s (`RealtimeTaskRef`s). 
/// This is used to store the `Task`s (and associated scheduler related data) 
/// that are runnable on a given core.
/// A queue is used for the round robin scheduler.
/// `Runqueue` implements `Deref` and `DerefMut` traits, which dereferences to `VecDeque`.
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
	/// Moves the `RealtimeTaskRef` at the given index in this `RunQueue` to the appropriate location in this `RunQueue`,
	/// and returns a cloned reference to the underlying `TaskRef`.
	/// Under the Rate Monotonic scheduling algorithm, periodic tasks are assigned priorities in order from the smallest period.
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
	pub fn add_task_to_any_runqueue_realtime(task: TaskRef, period: Option<usize>) -> Result<(), &'static str> {
        let rq = RunQueue::get_least_busy_runqueue()
            .or_else(|| RUNQUEUES.iter().next().map(|r| r.1))
            .ok_or("couldn't find any runqueues to add the task to!")?;

        rq.write().add_task(task, period)
	}

    /// Convenience method that adds the given `Task` reference to given core's runqueue.
	pub fn add_task_to_specific_runqueue_realtime(which_core: u8, task: TaskRef, period: Option<usize>) -> Result<(), &'static str> {
        RunQueue::get_runqueue(which_core)
            .ok_or("Couldn't get RunQueue for the given core")?
            .write()
            .add_task(task, period)
	}

	/// Inserts a `RealtimeTaskRef` at its proper position in the queue
	/// Under the RMS scheduling algorithm, tasks should be ordered in increasing value of their periods, with aperiodic tasks being placed at the back
	/// Thus, we will insert all `RealtimeTaskRef`s whose `period` is `None` and all `RealtimeTaskRef`s with a proper value for period will be place at the location where they belong in the sorted list
	fn insert_realtime_taskref_at_proper_location(&mut self, taskref: RealtimeTaskRef) {
		match taskref.period {
			None => self.push_back(taskref),
			Some(_) => {
				if self.is_empty() {
					self.push_back(taskref)
				} else {
					let mut index_to_insert: usize = 0;
					let mut found_index_to_insert = false;
					for (index, inserted_taskref) in self.iter().enumerate() {
						if taskref.has_smaller_period(inserted_taskref) {
							index_to_insert = index;
							found_index_to_insert = true;
							break;
						}
					}

					if found_index_to_insert {
						self.insert(index_to_insert, taskref);
					} else {
						self.push_back(taskref)
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
        debug!("Removing task from runqueue_priority {}, {:?}", self.core, task);
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
}