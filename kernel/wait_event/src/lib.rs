#![no_std]
#![feature(trait_alias)]

extern crate alloc;
#[macro_use] extern crate log;
extern crate irq_safety;
extern crate task;
extern crate scheduler;


use alloc::collections::VecDeque;
use irq_safety::MutexIrqSafe;
use task::TaskRef;


/// Errors that may occur while waiting on a waitqueue/condition/event.
#[derive(Debug)]
pub enum WaitError {
    NoCurrentTask,
    Interrupted,
    Timeout,
    SpuriousWakeup,
}

/// The closure type that can be used within a `WaitCondition`:
/// a parameterless function that returns a bool indicating whether the condition is met.
pub trait WaitConditionFn = Fn() -> bool;


/// A condition variable that allows multiple `Task`s to wait for a condition to be met,
/// upon which other `Task`s can notify them.  
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
        while !(self.condition_fn)() {
            self.wait_queue.wait()?;
            // trace!("wait_event:  woke up!");
        }
        Ok(())
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


/// A queue in which multiple `Task`s can wait for other `Task`s to notify them.
/// 
/// This can be shared across multiple `Task`s by wrapping it in an `Arc`. 
pub struct WaitQueue(MutexIrqSafe<VecDeque<TaskRef>>);

// ******************************************************************
// ************ IMPORTANT IMPLEMENTATION NOTE ***********************
// All modification of task runstates must be performed atomically with respect to
// the waitqueue itself. In other words, the inner lock on the waitqueue
// should be held for the full duration of all modifications to any task runstates
// for tasks that are being added to or removed from the waitqueue. 
// Otherwise, there could be interleavings that result in tasks not being notified properly
// or not actually being put to sleep when being placed on the waitqueue.
// ******************************************************************

impl WaitQueue {
    /// Create a new empty WaitQueue.
    pub fn new() -> WaitQueue {
        WaitQueue(MutexIrqSafe::new(VecDeque::with_capacity(4)))
    }

    /// Puts the current `Task` to sleep where it blocks on this `WaitQueue`
    /// until it is notified by another `Task`. 
    /// 
    /// If the `Task` wakes up spuriously (it is still on the waitqueue),
    /// it will be automatically put back to sleep until it is properly woken up. 
    /// Therefore, there is no need for the caller to check for spurious wakeups.
    /// 
    /// This function blocks until the `Task` is woken up through the notify mechanism.
    pub fn wait(&self) -> Result<(), WaitError> {
        let curr_task = task::get_my_current_task().ok_or(WaitError::NoCurrentTask)?;

        // The following must be done "atomically" (w.r.t. the waitqueue):
        // (1) add the current task to the waitqueue
        // (2) set the current task as blocked
        {
            let mut wq_locked = self.0.lock();
            wq_locked.push_back(curr_task.clone());
            // trace!("WaitQueue::wait():  putting task to sleep: {:?}\n    --> WQ: {:?}", curr_task, &*wq_locked);
            curr_task.block();
            // `wq_locked` is dropped
        }
        scheduler::schedule();

        // Here, we have been woken up.
        // We need to check if we were intentionally woken up via the waitqueue notify mechanism,
        // or if we accidentally woke up due to another reason.
        // trace!("WaitQueue::wait():  woke up!");

        // TODO: below, we should check for a timeout being exceeded
        loop {
            {
                let wq_locked = self.0.lock();
                if wq_locked.contains(curr_task) {
                    // spurious wake up
                    warn!("WaitQueue::wait():  task spuriously woke up, putting back to sleep: {:?}", curr_task);
                    curr_task.block();
                } else {
                    // intended wake up
                    break;
                }
            }
            scheduler::schedule();
        }
        
        // Here, we were woken up as expected (non-spuriously)
        Ok(())
    }

    /// Wake up one random `Task` that is waiting on this queue.
    /// # Return
    /// * returns `Ok(true)` if a `Task` was successfully woken up,
    /// * returns `Ok(false)` if there were no `Task`s waiting.
    pub fn notify_one(&self) -> bool {
        self.notify(None)
    }

    /// Wake up a specific `Task` that is waiting on this queue.
    /// # Return
    /// * returns `true` if the given `Task` was waiting and was woken up,
    /// * returns `false` if there was no such `Task` waiting.
    pub fn notify_specific(&self, task_to_wakeup: &TaskRef) -> bool {
        self.notify(Some(task_to_wakeup))
    }
    
    /// The internal routine for notifying / waking up tasks that are blocking on the waitqueue. 
    /// If specified, the given `task_to_wakeup` will be notified, 
    /// otherwise the first task on the waitqueue will be notified.
    fn notify(&self, task_to_wakeup: Option<&TaskRef>) -> bool {
        // trace!("  notify [top]: task_to_wakeup: {:?}", task_to_wakeup);

        let mut wq_locked = self.0.lock();
        let tref = if let Some(ttw) = task_to_wakeup {
            // find a specific task to wake up
            let index = wq_locked.iter().position(|t| t == ttw);
            index.and_then(|i| wq_locked.remove(i))
        } else {
            // just wake up the first task
            wq_locked.pop_front()
        };

        // trace!("  notify: chose task to wakeup: {:?}", tref);
        if let Some(t) = tref {
            // trace!("WaitQueue::notify():  unblocked task on waitqueue\n    --> WQ: {:?}", &*wq_locked);
            t.unblock();
            true
        } else {
            // trace!("WaitQueue::notify():  did nothing");
            false
        }

    }
}