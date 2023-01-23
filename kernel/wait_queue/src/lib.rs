#![allow(clippy::new_without_default)]
#![no_std]

extern crate alloc;
#[macro_use] extern crate log;
extern crate irq_safety;
extern crate task;
extern crate scheduler;


use alloc::collections::VecDeque;
use irq_safety::MutexIrqSafe;
use task::{TaskRef, RunState};


/// An object that holds a blocked `Task` 
/// that will be automatically unblocked upon drop.  
pub struct WaitGuard {
    task: TaskRef,
}
impl WaitGuard {
    /// Blocks the given `Task` and returns a new `WaitGuard` object
    /// that will automatically unblock that Task when it is dropped. 
    ///
    /// Returns an error if the task cannot be blocked;
    /// see [`task::Task::block()`] for more details.
    pub fn new(task: TaskRef) -> Result<WaitGuard, RunState> {
        task.block()?;
        Ok(WaitGuard { task })
    }

    /// Blocks the task guarded by this waitguard,
    /// which is useful to re-block a task after it spuriously woke up. 
    ///
    /// Returns an error if the task cannot be blocked;
    /// see [`task::Task::block()`] for more details.
    pub fn block_again(&self) -> Result<RunState, RunState> {
        self.task.block()
    }

    /// Returns a reference to the `Task` being blocked in this `WaitGuard`.
    pub fn task(&self) -> &TaskRef {
        &self.task
    }
}
impl Drop for WaitGuard {
    fn drop(&mut self) {
        self.task.unblock().unwrap();
    }
}


/// Errors that may occur while waiting on a waitqueue/condition/event.
#[derive(Debug, PartialEq)]
pub enum WaitError {
    NoCurrentTask,
    Interrupted,
    Timeout,
    SpuriousWakeup,
    CantBlockCurrentTask,
}

/// A queue in which multiple `Task`s can wait for other `Task`s to notify them.
/// 
/// This can be shared across multiple `Task`s by wrapping it in an `Arc`. 
pub struct WaitQueue(MutexIrqSafe<VecDeque<TaskRef>>);

// ******************************************************************
// ************ IMPORTANT IMPLEMENTATION NOTE ***********************
// All modification of task runstates must be performed atomically 
// with respect to adding or removing those tasks to/from the waitqueue itself.
// Otherwise, there could be interleavings that result in tasks not being notified properly,
// or not actually being put to sleep when being placed on the waitqueue, 
// or the task being switched away from after setting itself to blocked (when waiting) 
// but before it can release its lock on the waitqueue.
//    (Because once a task is blocked, it can never run again and thus 
//     has no chance to release its waitqueue lock, causing deadlock).
// Thus, we disable preemption (well, currently we disable interrupts) 
// AND hold the waitqueue lock while changing task runstate, 
// which ensures that once the task is blocked it will always release its waitqueue lock.
// ******************************************************************

impl WaitQueue {
    /// Create a new empty WaitQueue.
    pub fn new() -> WaitQueue {
        WaitQueue::with_capacity(4)
    }

    /// Create a new empty WaitQueue.
    pub fn with_capacity(initial_capacity: usize) -> WaitQueue {
        WaitQueue(MutexIrqSafe::new(VecDeque::with_capacity(initial_capacity)))
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
        self.wait_until(&|/* _ */| Some(()))
    }

    /// Similar to [`wait`](#method.wait), but this function blocks until the given
    /// `condition` closure returns `Some(value)`, and then returns that `value` inside `Ok()`.
    /// 
    /// The `condition` will be executed atomically with respect to the wait queue,
    /// which avoids the problem of a waiting task missing a "notify" from another task
    /// due to interleaving of instructions that may occur if the `condition` is checked 
    /// when the wait queue lock is not held. 
    /// 
    // /// The `condition` closure is invoked with one argument, an immutable reference to the waitqueue, 
    // /// to allow the closure to examine the condition of the waitqueue if necessary. 
    pub fn wait_until<R>(&self, condition: &dyn Fn(/* &VecDeque<TaskRef> */) -> Option<R>) -> Result<R, WaitError> {
        // Do the following atomically:
        // (1) Obtain the waitqueue lock
        // (2) Add the current task to the waitqueue
        // (3) Set the current task's runstate to `Blocked`
        // (4) Release the lock on the waitqueue.
        loop {
            {
                let mut wq_locked = self.0.lock();
                if let Some(ret) = condition(/* &wq_locked */) {
                    return Ok(ret);
                }
                task::with_current_task(|curr_task| {
                    // This is only necessary because we're using a non-Set waitqueue collection that allows duplicates
                    if !wq_locked.contains(curr_task) {
                        wq_locked.push_back(curr_task.clone());
                    } else {
                        warn!("WaitQueue::wait_until():  task was already on waitqueue (potential spurious wakeup?). {:?}", curr_task);
                    }
                    // trace!("WaitQueue::wait_until():  putting task to sleep: {:?}\n    --> WQ: {:?}", curr_task, &*wq_locked);
                    curr_task.block().map_err(|_| WaitError::CantBlockCurrentTask)
                }).map_err(|_| WaitError::NoCurrentTask)??;
            }
            scheduler::schedule();

            // Here, we have been woken up, so loop back around and check the condition again
            // trace!("WaitQueue::wait_until():  woke up!");
        }
    }

    /// Similar to [`wait_until`](#method.wait_until), but this function accepts a `condition` closure
    /// that can mutate its environment (a `FnMut`).
    pub fn wait_until_mut<R>(&self, condition: &mut dyn FnMut(/* &VecDeque<TaskRef> */) -> Option<R>) -> Result<R, WaitError> {
        // Do the following atomically:
        // (1) Obtain the waitqueue lock
        // (2) Add the current task to the waitqueue
        // (3) Set the current task's runstate to `Blocked`
        // (4) Release the lock on the waitqueue.
        loop {
            {
                let mut wq_locked = self.0.lock();
                if let Some(ret) = condition(/* &wq_locked */) {
                    return Ok(ret);
                }
                task::with_current_task(|curr_task| {
                    // This is only necessary because we're using a non-Set waitqueue collection that allows duplicates
                    if !wq_locked.contains(curr_task) {
                        wq_locked.push_back(curr_task.clone());
                    } else {
                        warn!("WaitQueue::wait_until():  task was already on waitqueue (potential spurious wakeup?). {:?}", curr_task);
                    }
                    // trace!("WaitQueue::wait_until():  putting task to sleep: {:?}\n    --> WQ: {:?}", curr_task, &*wq_locked);
                    curr_task.block().map_err(|_| WaitError::CantBlockCurrentTask)
                }).map_err(|_| WaitError::NoCurrentTask)??;
            }
            scheduler::schedule();

            // Here, we have been woken up, so loop back around and check the condition again
            // trace!("WaitQueue::wait_until():  woke up!");
        }
    }

    /// Wake up one random `Task` that is waiting on this queue.
    /// # Return
    /// * returns `true` if a `Task` was successfully woken up,
    /// * returns `false` if there were no `Task`s waiting.
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

    /// Wake up all `Task`s that are waiting on this queue.
    pub fn notify_all(&self) {
        for t in self.0.lock().drain(..) {
            if t.unblock().is_err() {
                warn!("WaitQueue::notify_all(): failed to unblock {:?}", t);
            }
        }
    }
    
    /// The internal routine for notifying / waking up tasks that are blocking on the waitqueue. 
    /// If specified, the given `task_to_wakeup` will be notified, 
    /// otherwise the first task on the waitqueue will be notified.
    fn notify(&self, task_to_wakeup: Option<&TaskRef>) -> bool {
        // trace!("  notify [top]: task_to_wakeup: {:?}", task_to_wakeup);

        // Do the following atomically:
        // (1) Obtain the waitqueue lock
        // (2) Choose a task and remove it from the waitqueue
        // (3) Set that task's runstate to `Runnable`
        // (4) Release the lock on the waitqueue.

        let mut wq_locked = self.0.lock();
        
        loop {
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
                if t.unblock().is_ok() {
                    return true;
                }
            } else {
                // trace!("WaitQueue::notify():  did nothing");
                return false;
            }
        }
    }
}
