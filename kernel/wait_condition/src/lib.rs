//! Simple condition variables that are convenience wrappers around wait queues.

#![no_std]
#![feature(trait_alias)]

// #[macro_use] extern crate log;
extern crate task;
extern crate wait_queue;

use task::TaskRef;
use wait_queue::{WaitQueue, WaitError};


/// The closure type that can be used within a `WaitCondition`:
/// a parameterless function that returns a bool indicating whether the condition is met.
pub trait WaitConditionFn = Fn() -> bool;


/// A condition variable that allows multiple `Task`s to wait for a condition to be met,
/// upon which other `Task`s can notify them.
/// This is effectively a convenience wrapper around `WaitQueue::wait_until()`.  
/// 
/// The condition is specified as an closure that returns a boolean: 
/// `true` if the condition has been met, `false` if not. 
/// 
/// The condition closure must be a regular `Fn` that can be repeatedly executed,
/// and should be cheap and quick to execute. 
/// Complicated logic should be kept outside of the condition function. 
/// 
/// This can be shared across multiple `Task`s by wrapping it in an `Arc`. 
pub struct WaitCondition<F: WaitConditionFn> {
    condition_fn: F,
    wait_queue: WaitQueue,
}

impl<F: Fn() -> bool> WaitCondition<F> {
    /// Create a new `WaitCondition` in which `Task`s can wait
    /// for a condition to be met, as defined by the given `condition_fn`.
    pub fn new(condition_fn: F) -> WaitCondition<F> {
        WaitCondition {
            condition_fn,
            wait_queue: WaitQueue::new(),
        }
    }

    /// Waits for the condition to be true in a blocking fashion 
    /// that puts the current `Task` to sleep until it is notified that the condition has been met. 
    /// 
    /// The design of `WaitCondition` prevents spurious wakeups; 
    /// Tasks are only allowed to If the `Task` wakes up spuriously (it is still on the waitqueue),
    /// it will be automatically put back to sleep until it is properly woken up. 
    /// Therefore, there is no need for the caller to check for spurious wakeups.
    /// 
    /// This function blocks until the `Task` is woken up through the notify mechanism.
    pub fn wait(&self) -> Result<(), WaitError> {
        if (self.condition_fn)() {
            return Ok(());
        }
        self.wait_queue.wait_until(&|| {
            if (self.condition_fn)() {
                Some(())
            } else {
                None
            }
        })
    }

    /// This function should be invoked after the wait condition has been met
    /// and you are ready to notify other waiting tasks.
    /// The condition function within this `WaitCondition` object will be run again to ensure it has been met. 
    /// 
    /// If the condition is met, it returns a `SatisfiedWaitCondition` object that can be used.
    /// to notify (wake up) the other tasks waiting on this `WaitCondition`.
    /// If the condition is not met, it returns `None`. 
    pub fn condition_satisfied(&self) -> Option<SatisfiedWaitCondition<F>> {
        if (self.condition_fn)() {
            Some(SatisfiedWaitCondition {
                inner: self,
            })
        } else {
            None
        }
    }
}


/// A type wrapper that guarantees a given condition has been met
/// before allowing one task to notify (wake up) other `Task`s waiting on a `WaitCondition`.
/// See the [`condition_satisfied()`](#WaitCondition.condition_satisfied) method.
pub struct SatisfiedWaitCondition<'wc, F: WaitConditionFn> {
    inner: &'wc WaitCondition<F>,
}
impl<'wc, F: WaitConditionFn> SatisfiedWaitCondition<'wc, F> {
    /// Wake up a random `Task` that is waiting on this condition.
    /// # Return
    /// * returns `Ok(true)` if a `Task` was successfully woken up,
    /// * returns `Ok(false)` if there were no `Task`s waiting.
    pub fn notify_one(&self) -> bool {
        self.inner.wait_queue.notify_one()
    }

    /// Wake up a specific `Task` that is waiting on this condition.
    /// # Return
    /// * returns `true` if the given `Task` was waiting and was successfully woken up,
    /// * returns `false` if there was no such `Task` waiting.
    pub fn notify_specific(&self, task_to_wakeup: &TaskRef) -> bool {
        self.inner.wait_queue.notify_specific(task_to_wakeup)
    }
}