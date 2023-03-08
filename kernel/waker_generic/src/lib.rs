//! A standalone, generic implementation of a Waker + Blocker pair -- a basic wait event.
//!
//! By standalone, we mean that this crate does not depend on tasking infrastructure
//! (e.g., the `task` crate) directly, meaning it is usable in the `task` crate.
//! This works by accepts generic actions (closures) for waking the [Waker](core::task::Waker)
//! and blocking the [Blocker].

#![no_std]

extern crate alloc;

use alloc::sync::Arc;
use spin::Mutex; 

/// Creates a new waker and blocker pair that are associated with each other.
///
/// # Arguments
/// * `wake_action`: the action that will be taken by the waker when it is ready
///    to wake up the blocker, i.e., allow it to stop blocking.
///
/// The blocker can be used to block a task until the waker is woken.
pub fn new_waker<W>(wake_action: W) -> (core::task::Waker, Blocker)
where
    W: Fn() + Send + Sync + 'static, // required by bounds on `core::task::Waker::from`
{
    let woken = Arc::new(Mutex::new(false));
    (
        core::task::Waker::from(Arc::new(Waker {
            woken: woken.clone(),
            wake_action,
        })),
        Blocker { woken },
    )
}

/// A blocker that blocks in a loop until the associated waker is woken.
///
/// To obtain a `Blocker` and its associated [`core::task::Waker`], call [`new_waker()`].
pub struct Blocker {
    /// Whether the waker has been woken.
    ///
    /// This is shared with the Waker associated with this Blocker.
    /// If true, this Blocker can cease blocking and return to its caller.
    woken: Arc<Mutex<bool>>,
}
impl Blocker {
    /// Performs the given `block_action` within this `Blocker`,
    /// looping until the associated waker is woken.
    ///
    /// If the waker was already woken prior to this function being called,
    /// it will return immediately.
    ///
    /// After this function returns, the inner state of the blocker+waker pair
    /// will have been reset to its initial "unwoken" state, enabling them to be re-used.
    /// In other words, this function can be called again, at which point it will
    /// block until the waker is woken again.
    ///
    /// # Arguments
    /// * `block_action`: a closure that is called in a loop until its woker is woken.
    ///    The return value `R` of the closure is any arbitrary value that will be held until
    ///    *after* the blocker has released the lock on its inner state shared with the waker.
    ///    * This can be used, for example, to yield the CPU to another task.
    pub fn block<B, R>(&self, block_action: B)
    where
        B: Fn() -> R,
    {
        // Care must be taken not to introduce race conditions.
        // After creating the waker, the wake condition must be checked to ensure
        // that it did not get woken prior to registering the waker;
        // otherwise, the waker will never be woken, and this function will loop forever.
        loop {
            let mut woken = self.woken.lock();
            if *woken {
                // Setting woken back to false enables this blocker+waker to be re-used.
                *woken = false;
                return;
            } else {
                // Temporarily disable preemption to ensure that if/when `block_action()`
                // disables this task, it keeps running long enough to return here and
                // drop the lock on `woken`.
                let result = {
                    let guard = preemption::hold_preemption_no_timer_disable();
                    let r = block_action();
                    drop(woken);
                    drop(guard);
                    r
                };
                drop(result);
            }
        }
    }
}

/// A waker that performs a specific action when woken.
#[derive(Debug)]
struct Waker<W> 
where
    W: Fn() + Send
{
    /// Whether the waker has been woken.
    ///
    /// This field ensures that [`Blocker::block`] can determine whether the associated waker
    /// was woken *prior* to [`Blocker::block`] blocking the task.
    /// Thus, this field cannot be an `AtomicBool`, because the lock must be held
    /// while blocking or unblocking the task, serving as a critical section.
    woken: Arc<Mutex<bool>>,
    /// The action to take upon waking this waker.
    wake_action: W,
}

impl<W> alloc::task::Wake for Waker<W>
where
    W: Fn() + Send
{
    fn wake(self: Arc<Self>) {
        self.wake_by_ref()
    }

    fn wake_by_ref(self: &Arc<Self>) {
        let mut woken = self.woken.lock();
        *woken = true;
        // Temporarily disable preemption to ensure that the lock on `woken`
        // is released expediently after `wake_action()` is done.
        // This is not technically required, but it prevents this waker task
        // from being scheduled out while still holding the `woken` lock.
        let guard = preemption::hold_preemption_no_timer_disable();
        (self.wake_action)();
        drop(woken);
        drop(guard);
    }
}
