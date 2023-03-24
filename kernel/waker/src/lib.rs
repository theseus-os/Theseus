//! A standard implementation of a Waker + Blocker pair -- a basic wait event.
//!
//! This crate directly By standalone, we mean that this crate does not depend on tasking infrastructure
//! (e.g., the `task` crate) directly, meaning it is usable in the `task` crate.
//! This works by accepts generic actions (closures) for waking the [Waker](core::task::Waker)
//! and blocking the [Blocker].

#![no_std]
#![feature(negative_impls)]

extern crate alloc;

use task::{ScheduleOnDrop, TaskRef};

/// Creates a new waker and blocker pair that are associated with each other.
///
/// The blocker can be used to block the current task until the waker is woken.
pub fn new_waker() -> (core::task::Waker, Blocker) {
    let curr_task = task::get_my_current_task() 
        .expect("waker::new_waker(): failed to get current task");
    let task_to_block = curr_task.clone();
    let wake_action = move || {
        let _ = curr_task.unblock();
    };
    let (waker, blocker_generic) = waker_generic::new_waker(wake_action);
    (
        waker,
        Blocker {
            blocker_generic,
            task_to_block,
        }
    )
}

/// A blocker that blocks until the associated waker is woken.
///
/// To obtain a `Blocker` and its associated [`core::task::Waker`], call [`new_waker()`].
///
/// `Blocker` will block the current task; thus, it doesn't implement [`Send`] or [`Sync`]
/// to ensure that it cannot be sent to other tasks.
pub struct Blocker {
    blocker_generic: waker_generic::Blocker,
    task_to_block: TaskRef,
}
impl !Send for Blocker {}
impl !Sync for Blocker {}

impl Blocker {
    /// Blocks the current task by putting it to sleep until the associated waker is woken.
    ///
    /// If the waker was already woken prior to this function being called,
    /// it will return immediately.
    ///
    /// After this function returns, the inner state of the blocker+waker pair
    /// will have been reset to its initial "unwoken" state, enabling them to be re-used.
    /// In other words, this function can be called again, at which point it will
    /// block until the waker is woken again.
    pub fn block(&self) {
        let block_action = || {
            let _ = self.task_to_block.block();
            ScheduleOnDrop { }
        };
        self.blocker_generic.block(block_action)
    }
}
