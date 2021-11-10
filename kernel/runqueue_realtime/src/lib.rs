#![no_std]

extern crate task;
extern crate irq_safety;

use task::TaskRef;
use irq_safety::RwLockIrqSafe;

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