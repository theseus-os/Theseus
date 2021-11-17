#![no_std]

extern crate task;
extern crate irq_safety;
extern crate alloc;
#[macro_use] extern crate log;
#[macro_use] extern crate lazy_static;

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

	/// TODO!!!
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

	/// TODO!!!
	pub fn get_runqueue(which_core: u8) -> Option<&'static RwLockIrqSafe<RunQueue>> {
		RUNQUEUES.get(&which_core)
	} 

	/// TODO!!!
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

	/// TODO!!!
	pub fn add_task_to_any_runqueue_realtime(task: TaskRef, period: Option<usize>) -> Result<(), &'static str> {
        let rq = RunQueue::get_least_busy_runqueue()
            .or_else(|| RUNQUEUES.iter().next().map(|r| r.1))
            .ok_or("couldn't find any runqueues to add the task to!")?;

        rq.write().add_task(task, period)
	}

	/// TODO!!!
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
					for inserted_taskref in self.iter() {
						if taskref.has_smaller_period(inserted_taskref) {
							found_index_to_insert = true;
							break;
						}

						index_to_insert += 1;
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

	/// TODO!!!
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


	/// TODO!!!
	pub fn remove_task(&mut self, task: &TaskRef) -> Result<(), &'static str> {
		self.remove_internal(task)
	}

	/// TODO!!!
	pub fn remove_task_from_all(task: &TaskRef) -> Result<(), &'static str> {
        for (_core, rq) in RUNQUEUES.iter() {
            rq.write().remove_task(task)?;
        }
        Ok(())
	}
}