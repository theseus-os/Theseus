#![no_std]
#![feature(trait_alias)]

extern crate alloc;
#[macro_use] extern crate log;
extern crate spin;
extern crate task;
extern crate scheduler;


use alloc::collections::VecDeque;
use spin::Mutex;
use task::TaskRef;


/// Errors that may occur while waiting on a waitqueue/condition/event.
#[derive(Debug)]
pub enum WaitError {
    NoCurrentTask,
    Interrupted,
    Timeout,
    SpuriousWakeup,
}



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
            trace!("wait_event:  woke up!");
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
pub struct WaitQueue(Mutex<VecDeque<TaskRef>>);

impl WaitQueue {
    /// Create a new empty WaitQueue.
    pub fn new() -> WaitQueue {
        WaitQueue(Mutex::new(VecDeque::with_capacity(4)))
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
        self.0.lock().push_back(curr_task.clone());
        trace!("WaitQueue::wait():  putting task to sleep: {:?}", curr_task);
        curr_task.block();
        scheduler::schedule();

        // Here, we have been woken up.
        // We need to check if we were intentionally woken up via the waitqueue notify mechanism,
        // or if we accidentally woke up due to another reason.

        // TODO: here check for a timeout being exceeded
        while self.0.lock().contains(curr_task) {
            warn!("WaitQueue::wait():  task spuriously woke up, putting back to sleep: {:?}", curr_task);
            curr_task.block();
            scheduler::schedule();
        }
        
        // Here, we were woken up as expected (non-spuriously)
        Ok(())
    }

    /// Wake up a random `Task` that is waiting on this queue.
    /// # Return
    /// * returns `Ok(true)` if a `Task` was successfully woken up,
    /// * returns `Ok(false)` if there were no `Task`s waiting.
    pub fn notify_one(&self) -> bool {
        if let Some(tref) = self.0.lock().pop_front() {
            tref.unblock();
            true
        } else {
            trace!("WaitQueue::notify_one():  waitqueue was empty");
            false
        }
    }

    /// Wake up a specific `Task` that is waiting on this queue.
    /// # Return
    /// * returns `true` if the given `Task` was waiting for the event and was successfully woken up,
    /// * returns `false` if there was no such `Task` waiting.
    pub fn notify_specific(&self, task_to_wakeup: &TaskRef) -> bool {
        let tref = { 
            let mut wq = self.0.lock();
            let index = wq.iter().position(|t| t == task_to_wakeup);
            index.and_then(|i| wq.remove(i))
        };
        if let Some(t) = tref {
            t.unblock();
            true
        } else {
            trace!("WaitQueue::notify_specific():  waitqueue didn't contain task {:?}", task_to_wakeup);
            false
        }
    }
}