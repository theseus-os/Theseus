//! Provides APIs for tasks to sleep for specified time durations.
//!
//! Key functions:
//! * The [`sleep`] function delays the current task for a given number of ticks.
//! * The [`sleep_until`] function delays the current task until a specific moment in the future.
//! * The [`sleep_periodic`] function allows for tasks to be delayed for periodic intervals
//!  of time and can be used to implement a period task.
//!
//! TODO: use regular time-keeping abstractions like Duration and Instant.

#![no_std]
extern crate task;
extern crate irq_safety;
extern crate alloc;
#[macro_use] extern crate lazy_static;
extern crate scheduler;

use core::sync::atomic::{Ordering, AtomicUsize};
use alloc::collections::binary_heap::BinaryHeap;
use irq_safety::MutexIrqSafe;
use task::{get_my_current_task, TaskRef};

/// Contains the `TaskRef` and the associated wakeup time for an entry in DELAYED_TASKLIST.
#[derive(Clone, Eq, PartialEq)]
struct SleepingTaskNode {
    resume_time: usize,
    taskref: TaskRef,
}

// The priority queue depends on `Ord`.
// Explicitly implement the trait so the queue becomes a min-heap
// instead of a max-heap.
impl Ord for SleepingTaskNode {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        // Notice that the we flip the ordering on resume_time.
        // In case of a tie we compare taskids - this step is necessary
        // to make implementations of `PartialEq` and `Ord` consistent.
        other.resume_time.cmp(&self.resume_time)
            .then_with(|| self.taskref.id.cmp(&other.taskref.id))
    }
}

// `PartialOrd` needs to be implemented as well.
impl PartialOrd for SleepingTaskNode {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

lazy_static! {
    /// List of all delayed tasks in the system
    /// Implemented as a min-heap of `SleepingTaskNode` sorted in increasing order of `resume_time`
    static ref DELAYED_TASKLIST: MutexIrqSafe<BinaryHeap<SleepingTaskNode>> 
        = MutexIrqSafe::new(BinaryHeap::new());
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


/// Helper function adds the id associated with a TaskRef to the list of delayed task.
/// If the resume time is less than the current earliest resume time, then update it.
fn add_to_delayed_tasklist(new_node: SleepingTaskNode) {
    let SleepingTaskNode { resume_time, .. } = new_node;
    DELAYED_TASKLIST.lock().push(new_node);
    
    let next_unblock_time = NEXT_DELAYED_TASK_UNBLOCK_TIME.load(Ordering::SeqCst);
    if resume_time < next_unblock_time {
        NEXT_DELAYED_TASK_UNBLOCK_TIME.store(resume_time, Ordering::SeqCst);
    }
}

/// Remove the next task from the delayed task list and unblock that task
fn remove_next_task_from_delayed_tasklist() {
    let mut delayed_tasklist = DELAYED_TASKLIST.lock();
    if let Some(SleepingTaskNode { taskref, .. }) = delayed_tasklist.pop() {
        taskref.unblock();

        match delayed_tasklist.peek() {
            Some(SleepingTaskNode { resume_time, .. }) => 
                NEXT_DELAYED_TASK_UNBLOCK_TIME.store(*resume_time, Ordering::SeqCst),
            None => NEXT_DELAYED_TASK_UNBLOCK_TIME.store(usize::MAX, Ordering::SeqCst),
        }
    }
}

/// Remove all tasks that have been delayed but are able to be unblocked now,
/// the current tick count is provided by the system's interrupt tick count.
pub fn unblock_sleeping_tasks() {
    let ticks = TICK_COUNT.load(Ordering::SeqCst);
    while ticks > NEXT_DELAYED_TASK_UNBLOCK_TIME.load(Ordering::SeqCst) {
        remove_next_task_from_delayed_tasklist();
    }
}

/// Blocks the current task by putting it to sleep for `duration` ticks.
pub fn sleep(duration: usize) {
    let current_tick_count = TICK_COUNT.load(Ordering::SeqCst);
    let resume_time = current_tick_count + duration;

    let current_task = get_my_current_task().unwrap().clone();
    // Add the current task to the delayed tasklist and then block it.
    add_to_delayed_tasklist(SleepingTaskNode{taskref: current_task.clone(), resume_time});
    current_task.block();
    scheduler::schedule();
}

/// Blocks the current task by putting it to sleep until a specific tick count is reached,
/// given by `resume_time`.
pub fn sleep_until(resume_time: usize) {
    let current_tick_count = TICK_COUNT.load(Ordering::SeqCst);

    if resume_time > current_tick_count {
        sleep(resume_time - current_tick_count);
    }
}

/// Blocks the current task for a fixed time `period`, which starts from the given `last_resume_time`.
pub fn sleep_periodic(last_resume_time: &AtomicUsize, period: usize) {
    let new_resume_time = last_resume_time.fetch_add(period, Ordering::SeqCst) + period;
    sleep_until(new_resume_time);
}
