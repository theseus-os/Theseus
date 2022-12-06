#![no_std]

extern crate alloc;

use alloc::{sync::Arc, task::Wake};
use core::{
    future::Future,
    task::{Context, Poll},
};
use spin::Mutex;

pub use futures::{future, select_biased, FutureExt};

/// Executes a future.
pub fn execute<F, O>(future: F) -> O
where
    F: Future<Output = O>,
{
    // Pin the future onto the stack. This works because we don't send it anywhere.
    futures::pin_mut!(future);
    let activated = Arc::new(Mutex::new(false));
    let task = task::get_my_current_task().unwrap();
    let waker = core::task::Waker::from(Arc::new(Waker {
        activated: activated.clone(),
        task: task.clone(),
    }));
    let mut context = Context::from_waker(&waker);

    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(output) => return output,
            Poll::Pending => {
                let mut activated = activated.lock();
                if !*activated {
                    let _ = task.block();
                    drop(activated);
                    scheduler::schedule();
                } else {
                    *activated = false;
                    drop(activated);
                }
            }
        }
    }
}

/// A waker that unblocks the given task when awoken.
struct Waker {
    /// Whether the waker has been activated.
    ///
    /// This field ensures `execute` detects if the waker was activated prior to
    /// `execute` blocking the task. The field cannot be an atomic as the lock
    /// must be held while blocking or unblocking the task.
    activated: Arc<Mutex<bool>>,
    task: task::TaskRef,
}

impl Wake for Waker {
    fn wake(self: Arc<Self>) {
        let mut activated = self.activated.lock();
        *activated = true;
        let _ = self.task.unblock();
        drop(activated);
    }
}
