//! This crate contains the `RunQueue` structure, which is essentially a list of Tasks
//! that it used for scheduling purposes.
//! 

#![no_std]
#![feature(alloc)]

extern crate alloc;
extern crate irq_safety;
extern crate atomic_linked_list;
extern crate task;

#[cfg(single_simd_task_optimization)]
extern crate single_simd_task_optimization;

use irq_safety::{RwLockIrqSafe};
use task::{TaskRef};



/// A list of references to `Task`s (`TaskRef`s) 
/// that is used to store the `Task`s that are runnable on a given core. 
pub trait RunQueueTrait {
    // Static method signature; `Self` refers to the implementor type.
    fn init(which_core: u8) -> Result<(), &'static str>;

    fn get_runqueue(which_core: u8) -> Option<&'static RwLockIrqSafe<Self>>;

    fn get_least_busy_core() -> Option<u8>;

    fn get_least_busy_runqueue() -> Option<&'static RwLockIrqSafe<Self>>;

    fn add_task_to_any_runqueue(task: TaskRef) -> Result<(), &'static str>;

    fn add_task_to_specific_runqueue(which_core: u8, task: TaskRef) -> Result<(), &'static str>;

    fn add_task(&mut self, task: TaskRef) -> Result<(), &'static str>;

    fn get(&self, index: usize) -> Option<&TaskRef>;

    fn remove_internal(&mut self, task: &TaskRef) -> Result<(), &'static str>;

    fn remove_task(&mut self, task: &TaskRef) -> Result<(), &'static str>;

    fn remove_task_from_all(task: &TaskRef) -> Result<(), &'static str>;

}


