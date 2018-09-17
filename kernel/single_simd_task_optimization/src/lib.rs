//! Implements the performance optimization that allows a SIMD-enabled Task
//! to skip saving/restoring SIMD registers when context switching, 
//! if and only if it is the only SIMD-enabled Task on its entire core. 
//! 
//! See the documentation of the 
//! [`simd_personality`](../simd_personality/index.html#context-switching) crate
//! for further discussion.
//! 

#![no_std]

#[macro_use] extern crate log;
extern crate task;

use task::TaskRef;

/// This function should be called when there was a new SIMD-enabled Task
/// that was added to the list of Tasks eligible to run on the given core. 
/// # Arguments
/// `tasks_on_core` is an Iterator over all of the `TaskRef`s that 
/// are eligible to run on the given core `which_core`.
pub fn simd_tasks_added_to_core<'t, I>(tasks_on_core: I, _which_core: u8) 
	where I: Iterator<Item = &'t TaskRef>
{
	let num_simd_tasks = &tasks_on_core
		.filter(|taskref| taskref.read().simd)
		.count();
	warn!("simd_tasks_added_to_core(): core {} now has {} SIMD tasks total.", 
		_which_core, num_simd_tasks);

	match num_simd_tasks {
		0 => {
			error!("BUG: simd_tasks_added_to_core(): there were no SIMD tasks on this core.");
		}
		1 => {
			// Here, we previously had 0 SIMD tasks, and now we have 1. 
			// TODO: So, convert that one SIMD Task into a non-SIMD Context
			// We have to do this conversion here because all SIMD Tasks start out
			// using the SIMD-enabled Context by default, just to ensure correcntess.
		}
		2 => {
			// Here, we previously had 1 SIMD task, and now we have 2. 
			// TODO: Convert all SIMD tasks back to their default state of using the SIMD Context.
		}
		_ => {
			// Here, we had more than one SIMD task, and now we still have more than 1. 
			// So, those tasks have already been converted back to using the regular SIMD Context,
			// therefore, we do not need to do anything
		}
	}
}


/*
/// This function should be called when there was a new SIMD-enabled Task
/// that was added to the list of Tasks eligible to run on the given core. 
/// # Arguments
/// `tasks_on_core` is an Iterator over all of the `TaskRef`s that 
/// are eligible to run on the given core `which_core`.
pub fn simd_tasks_removed_from_core<I>(tasks_on_core: I, _which_core: u8) 
	where I: IntoIterator<Item = TaskRef>
{
	let num_simd_tasks = tasks_on_core
		.into_iter()
		.filter(|taskref| taskref.read().simd)
		.count();
	warn!("simd_tasks_removed_from_core(): core {} now has {} SIMD tasks total.", 
		_which_core, num_simd_tasks);
}
*/