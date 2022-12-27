#![feature(negative_impls)]
#![no_std]

extern crate alloc;

use alloc::sync::Arc;
use mutex_sleep::MutexSleep as Mutex;
use task::{get_my_current_task, TaskRef};

/// Creates a new waker and blocker.
///
/// The blocker can be used to block the current task until the waker is woken.
pub fn waker() -> (core::task::Waker, Blocker) {
    let activated = Arc::new(Mutex::new(false));
    (
        core::task::Waker::from(Arc::new(Waker {
            activated: activated.clone(),
            task: get_my_current_task().expect("failed to get current task"),
        })),
        Blocker { inner: activated },
    )
}

/// A blocker that blocks the current task until the associated waker is woken.
pub struct Blocker {
    inner: Arc<Mutex<bool>>,
}

// Blocker blocks the current thread and thus shouldn't be sent to other
// threads.

impl !Send for Blocker {}
impl !Sync for Blocker {}

impl Blocker {
    /// Blocks the current thread until the associated waker is woken.
    ///
    /// If the waker was woken prior to this function being called, it will
    /// return immediately.
    ///
    /// Care must be taken not to introduce race conditions. After registering
    /// the waker, the wake condition must be checked to ensure it did not
    /// complete prior to registering the waker. Otherwise, the waker will never
    /// be woken, and this function will block forever.
    pub fn block(&self) {
        let task = get_my_current_task().expect("failed to get current task");
        loop {
            let mut activated = self.inner.lock().expect("failed to lock waker mutex");
            if *activated {
                *activated = false;
                break;
            } else {
                let _ = task.block();
                drop(activated);
                scheduler::schedule();
            }
        }
    }
}

/// A waker that unblocks the given task when awoken.
#[derive(Debug)]
struct Waker {
    /// Whether the waker has been activated.
    ///
    /// This field ensures `execute` detects if the waker was activated prior to
    /// `execute` blocking the task. The field cannot be an atomic as the lock
    /// must be held while blocking or unblocking the task.
    activated: Arc<Mutex<bool>>,
    task: TaskRef,
}

impl alloc::task::Wake for Waker {
    fn wake(self: Arc<Self>) {
        let mut activated = self.activated.lock().expect("failed to lock waker mutex");
        *activated = true;
        let _ = self.task.unblock();
        drop(activated);
    }
}
