#![no_std]

extern crate alloc;

use alloc::collections::VecDeque;
use task::TaskRef;

/// A mutex.
///
/// The implementation is based on [a Princeton University lecture][lecture].
///
/// [lecture]: https://www.cs.princeton.edu/courses/archive/fall16/cos318/lectures/6.MutexImplementation.pdf
#[derive(Debug, Default)]
pub struct Mutex {
    /// The inner state of a mutex.
    ///
    /// Using an IRQ safe mutex ensures even low priority tasks are able to
    /// complete their critical section. If preemption was enabled and a low
    /// priority task was preempted while holding onto the state, deadlock will
    /// occur if there are enough high priority tasks to not reschedule the
    /// low priority task, and one of the high priority task also tries to
    /// acquire the state.
    state: irq_safety::MutexIrqSafe<State>,
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
            state: irq_safety::MutexIrqSafe::new(State::new()),
        }
    }

    /// Locks the mutex, blocking the current thread until it is available.
    pub fn lock(&self) {
        let mut state = self.state.lock();

        if !state.is_locked {
            state.is_locked = true;
            return;
        }

        let current_task = task::get_my_current_task()
            .expect("raw_mutex::Mutex::lock(): couldn't get current task");
        state.queue.push_back(current_task);
        current_task.block();

        drop(state);
        scheduler::schedule();

        // NOTE: We only reach here after the thread has been unblocked by
        // another thread.
    }

    /// Unlocks the mutex.
    ///
    /// # Safety
    ///
    /// Behavior is undefined if the current thread does not actually hold the
    /// mutex.
    pub unsafe fn unlock(&self) {
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

    /// Attempts to lock the mutex without blocking, returning whether it was
    /// successfully acquired or not.
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
