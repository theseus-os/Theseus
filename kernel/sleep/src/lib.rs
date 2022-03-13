//! This crate provides an API that can be used to put tasks to sleep for specified periods of time.
//! The `sleep` function will delay the current task for a given number of ticks.
//! The `sleep_until` function will delay the current task until a specific moment in the future.
//! The `sleep_periodic` function allows for tasks to be delayed for periodic intervals of time and can be used to implement a period task.
//!

#![no_std]
extern crate task;
extern crate irq_safety;
extern crate priority_queue;
extern crate hashbrown;
#[macro_use] extern crate lazy_static;
extern crate scheduler;

use core::sync::atomic::{Ordering, AtomicUsize};
use core::cmp::Reverse;
use priority_queue::priority_queue::PriorityQueue;
use hashbrown::hash_map::DefaultHashBuilder;
use irq_safety::MutexIrqSafe;
use task::get_my_current_task;

lazy_static! {
    /// List of all delayed tasks in the system
    /// Implemented as a priority queue where the key is the unblocking time and the value is the id of the task
    static ref DELAYED_TASKLIST: MutexIrqSafe<PriorityQueue<usize, Reverse<usize>, DefaultHashBuilder>> = MutexIrqSafe::new(PriorityQueue::with_default_hasher());
}

/// Keeps track of the next task that needs to unblock, by default, it is the maximum time
static NEXT_DELAYED_TASK_UNBLOCK_TIME : AtomicUsize = AtomicUsize::new(usize::MAX);

/// This variable will track the number of ticks elapsed on the system to keep track of time
static TICK_COUNT: AtomicUsize = AtomicUsize::new(0);

/// Returns the current time in ticks
pub fn get_current_time_in_ticks() -> usize {
    TICK_COUNT.load(Ordering::SeqCst)
}

/// Update the current tick count
/// Used as a callback in the systick handler
pub fn increment_tick_count() {
    TICK_COUNT.fetch_add(1, Ordering::SeqCst);
}


/// Helper function adds the id associated with a TaskRef to the list of delayed tasks with priority equal to the time when the task must resume work
/// If the resume time is less than the current earliest resume time, we will update it
fn add_to_delayed_tasklist(taskid: usize, resume_time: usize) {
    DELAYED_TASKLIST.lock().push(taskid, Reverse(resume_time));
    
    let next_unblock_time = NEXT_DELAYED_TASK_UNBLOCK_TIME.load(Ordering::SeqCst);
    if resume_time < next_unblock_time {
        NEXT_DELAYED_TASK_UNBLOCK_TIME.store(resume_time, Ordering::SeqCst);
    }
}

/// Remove the next task from the delayed task list and unblock that task
fn remove_next_task_from_delayed_tasklist() {
    let mut delayed_tasklist = DELAYED_TASKLIST.lock();
    if let Some((taskid, _resume_time)) = delayed_tasklist.pop() {
    if let Some(task) = task::TASKLIST.lock().get(&taskid) {
        task.unblock();
    }

    match delayed_tasklist.peek() {
        Some((_new_taskid, Reverse(new_resume_time))) => NEXT_DELAYED_TASK_UNBLOCK_TIME.store(*new_resume_time, Ordering::SeqCst),
        None => NEXT_DELAYED_TASK_UNBLOCK_TIME.store(usize::MAX, Ordering::SeqCst),
    }
    }
}

/// Remove all tasks that have been delayed but are able to be unblocked now, the current tick count is provided by the system's timekeeper
pub fn unblock_delayed_tasks() {
    let ticks = TICK_COUNT.load(Ordering::SeqCst);
    while ticks > NEXT_DELAYED_TASK_UNBLOCK_TIME.load(Ordering::SeqCst) {
        remove_next_task_from_delayed_tasklist();
    }
}

/// Put the current task to sleep for `duration` ticks
pub fn sleep(duration: usize) {
    let current_tick_count = TICK_COUNT.load(Ordering::SeqCst);
    let resume_time = current_tick_count + duration;

    if let Some(current_task) = get_my_current_task() {
        // block current task and add it to the delayed tasklist
        current_task.block();
        let taskid = current_task.id;
        add_to_delayed_tasklist(taskid, resume_time);
        scheduler::schedule();
    }
}

/// Put the task to sleep until a specific tick count is reached, represented by `resume_time`
pub fn sleep_until(resume_time: usize) {
    let current_tick_count = TICK_COUNT.load(Ordering::SeqCst);

    // check that the resume time is greater than or equal to the current time, only then put it to sleep for the difference in those times
    // else do nothing
    if resume_time >= current_tick_count {
        sleep(resume_time - current_tick_count);
    }
}

/// Delay the current task for a fixed time period after the time in ticks specified by last_resume_time
/// Then we will update last_resume_time to its new value by adding the time period to its old value
pub fn sleep_periodic(last_resume_time: & AtomicUsize, period_length: usize) {
    let new_resume_time = last_resume_time.fetch_add(period_length, Ordering::SeqCst) + period_length;

    sleep_until(new_resume_time);
}

