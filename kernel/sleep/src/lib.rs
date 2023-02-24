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
extern crate time;
extern crate crossbeam_utils;

use core::task::Waker;
use alloc::collections::binary_heap::BinaryHeap;
use irq_safety::MutexIrqSafe;
use task::{get_my_current_task, TaskRef, RunState};
use crossbeam_utils::atomic::AtomicCell;
use time::{now, Instant, Monotonic};

pub use time::Duration;

/// Contains the `TaskRef` and the associated wakeup time for an entry in DELAYED_TASKLIST.
#[derive(Clone)]
struct SleepingTaskNode {
    resume_time: Instant,
    action: Action,
}

#[derive(Clone)]
enum Action {
    Sync(TaskRef),
    Async(Waker),
}

impl Action {
    fn act(self) {
        match self {
            Action::Sync(task) => {
                task.unblock().expect("failed to unblock sleeping task");
            },
            Action::Async(waker) => waker.wake(),
        }
    }
}

impl Eq for SleepingTaskNode {}

impl PartialEq for SleepingTaskNode {
    fn eq(&self, other: &Self) -> bool {
        self.resume_time == other.resume_time
    }
}

// The priority queue depends on `Ord`.
// Explicitly implement the trait so the queue becomes a min-heap
// instead of a max-heap.
impl Ord for SleepingTaskNode {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        // Notice that the we flip the ordering on resume_time.
        other.resume_time.cmp(&self.resume_time)
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
static NEXT_DELAYED_TASK_UNBLOCK_TIME: AtomicCell<Instant> = AtomicCell::new(Instant::MAX);

/// Helper function adds the id associated with a TaskRef to the list of delayed task.
/// If the resume time is less than the current earliest resume time, then update it.
fn add_to_delayed_tasklist(new_node: SleepingTaskNode) {
    let SleepingTaskNode { resume_time, .. } = new_node;
    DELAYED_TASKLIST.lock().push(new_node);
    
    let next_unblock_time = NEXT_DELAYED_TASK_UNBLOCK_TIME.load();
    if resume_time < next_unblock_time {
        NEXT_DELAYED_TASK_UNBLOCK_TIME.store(resume_time);
    }
}

/// Remove the next task from the delayed task list and unblock that task
fn remove_next_task_from_delayed_tasklist() {
    let mut delayed_tasklist = DELAYED_TASKLIST.lock();
    if let Some(SleepingTaskNode { action, .. }) = delayed_tasklist.pop() {
        action.act();

        match delayed_tasklist.peek() {
            Some(SleepingTaskNode { resume_time, .. }) => 
                NEXT_DELAYED_TASK_UNBLOCK_TIME.store(*resume_time),
            None => NEXT_DELAYED_TASK_UNBLOCK_TIME.store(Instant::MAX),
        }
    }
}

/// Remove all tasks that have been delayed but are able to be unblocked now,
/// the current tick count is provided by the system's interrupt tick count.
pub fn unblock_sleeping_tasks() {
    let time = now::<Monotonic>();
    while time > NEXT_DELAYED_TASK_UNBLOCK_TIME.load() {
        remove_next_task_from_delayed_tasklist();
    }
}

/// Blocks the current task by putting it to sleep for `duration` ticks.
///
/// Returns the current task's run state if it can't be blocked.
pub fn sleep(duration: Duration) -> Result<(), RunState> {
    let current_time = now::<Monotonic>();
    let resume_time = current_time + duration;

    let current_task = get_my_current_task().unwrap();
    // Add the current task to the delayed tasklist and then block it.
    add_to_delayed_tasklist(SleepingTaskNode{action: Action::Sync(current_task.clone()), resume_time});
    current_task.block()?;
    scheduler::schedule();
    Ok(())
}

/// Blocks the current task by putting it to sleep until a specific tick count is reached,
/// given by `resume_time`.
///
/// Returns the current task's run state if it can't be blocked.
pub fn sleep_until(resume_time: Instant) -> Result<(), RunState> {
    let current_time = now::<Monotonic>();

    if resume_time > current_time {
        sleep(resume_time - current_time)?;
    }
    
    Ok(())
}

/// Asynchronous sleep methods that operate on wakers.
pub mod future {
    use core::task::Poll;
    use super::*;

    /// Wakes up the waker after the specified duration.
    pub fn sleep(duration: Duration, waker: Waker) {
        let current_time = now::<Monotonic>();
        let resume_time = current_time + duration;

        add_to_delayed_tasklist(
            SleepingTaskNode {
                action: Action::Async(waker),
                resume_time,
            }
        );
    }

    /// Wakes up the waker at the specified time.
    pub fn sleep_until(resume_time: Instant, waker: &Waker) -> Poll<()> {
        let current_time = now::<Monotonic>();

        if let Some(duration) = resume_time.checked_duration_since(current_time) {
            sleep(duration, waker.clone());
            Poll::Pending
        } else {
            Poll::Ready(())
        }
    }
}