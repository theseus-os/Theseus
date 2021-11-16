#![no_std]

extern crate task;
extern crate irq_safety;
extern crate alloc;
#[macro_use] extern crate log;
#[macro_use] extern crate lazy_static;

use task::TaskRef;
use irq_safety::RwLockIrqSafe;
use core::ops::{Deref, DerefMut};
use alloc::collections::binary_heap::BinaryHeap;

/// A cloneable reference to a `Taskref` that exposes more methods
/// related to task scheduling
/// 
/// Each `RealtimeTaskRef` contains additional information on top a `TaskRef` object
/// In the case of realtime scheduling, we will be using the RMS algorithm,
/// thus, it is necessary to know whether a task is periodic.
/// If so, the field `period` will contain the period as an integer wrapped in a `Some` object.
/// If the task is aperiodic, `period` will contain the value `None`
/// `RealtimeTaskRef` implements `Deref` and `DerefMut` traits, which dereferences to `TaskRef`
#[derive(Debug, Clone, PartialEq, Eq)]
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
			Some(period) => true,
			None => false,
		}
	}
}

pub struct RunQueue;


impl RunQueue {
	/// TODO!!!
	pub fn init(which_core: u8) -> Result<(), &'static str> {
		Ok(())
	}

	/// TODO!!!
	pub fn get_runqueue(which_core: u8) -> Option<&'static RwLockIrqSafe<RunQueue>> {
		None
	} 

	/// TODO!!!
	pub fn get_least_busy_core() -> Option<u8> {
		None
	}

	/// TODO!!!
	pub fn add_task_to_any_runqueue(task: TaskRef) -> Result<(), &'static str> {
		Ok(())
	}

	/// TODO!!!
	pub fn add_task_to_specific_runqueue(which_core: u8, task: TaskRef) -> Result<(), &'static str> {
		Ok(())
	}

	/// TODO!!!
	pub fn remove_task(&mut self, task: &TaskRef) -> Result<(), &'static str> {
		Ok(())
	}

	/// TODO!!!
	pub fn remove_task_from_all(task: &TaskRef) -> Result<(), &'static str> {
		Ok(())
	}
}