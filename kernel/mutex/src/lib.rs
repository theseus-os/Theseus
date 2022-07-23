//! This crate contains a mutex implementation.
//!
//! The implementation is based on [https://www.cs.princeton.edu/courses/archive/fall16/cos318/lectures/6.MutexImplementation.pdf].

#![no_std]

extern crate alloc;

use alloc::collections::VecDeque;
use task::TaskRef;

#[derive(Debug, Default)]
pub struct Mutex {
    state: spin::Mutex<State>,
}

#[derive(Clone, Debug, Default)]
struct State {
    is_locked: bool,
    queue: VecDeque<&'static TaskRef>,
}

impl State {
    // TODO: Make const.
    pub fn new() -> Self {
        Self {
            is_locked: false,
            queue: VecDeque::new(),
        }
    }
}

impl Mutex {
    // TODO: Make const.
    pub fn new() -> Self {
        Self {
            state: spin::Mutex::new(State::new()),
        }
    }

    pub fn lock(&self) {
        let mut state = self.state.lock();

        if !state.is_locked {
            state.is_locked = true;
            return;
        }

        let current_task = task::get_my_current_task().unwrap();
        state.queue.push_back(current_task);
        current_task.block();

        drop(state);
        scheduler::schedule();

        // NOTE: We only return from the function when the mutex is released by
        // another thread.
    }

    pub fn unlock(&self) {
        let mut state = self.state.lock();
        debug_assert!(
            state.is_locked,
            "attempted to unlock an already unlocked mutex"
        );
        if let Some(task) = state.queue.pop_front() {
            task.unblock();
        } else {
            state.is_locked = false;
        }
    }

    pub fn try_lock(&self) -> bool {
        let mut state = self.state.lock();
        if state.is_locked {
            false
        } else {
            state.is_locked = true;
            true
        }
    }
}
