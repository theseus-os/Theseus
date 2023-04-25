//! Defines the [`SchedulerPolicy`] trait, an abstraction of scheduler policies.

#![no_std]

use mutex_preemption::RwLockPreempt;
use task::TaskRef;

pub trait AsSchedulerPolicy: Send + Sync {
    fn as_scheduler_policy<A, B, C>(&self) -> &dyn SchedulerPolicy<Runqueue = A, RunqueueId = B, RunqueueTaskRef = C>;
}

/// An abstraction of a scheduler policy.
///
/// A scheduler policy can be registered using [`task::set_scheduler_policy()`]
/// and will be used by future invocations of the `schedule()` routine.
pub trait SchedulerPolicy {
    /// The type of the runqueue(s) stored in this scheduler.
    type Runqueue;
    /// A unique identifier for a runqueue in this scheduler, e.g.,
    /// a CPU ID if there is one runqueue per CPU.
    type RunqueueId;
    /// The type of task reference stored in this scheduler's runqueue(s).
    ///
    /// This is typically a wrapper around a [`TaskRef`]
    /// and should implement a simple conversion from a [`TaskRef`].
    type RunqueueTaskRef: From<TaskRef>;

    /// Initializes a runqueue with the given runqueue ID in this scheduler.
    fn init_runqueue(&self, rq_id: Self::RunqueueId) -> Result<(), RunqueueError>;

    /// Returns the next task that should be scheduled in,
    /// or `None` if there are no runnable tasks.
    ///
    /// This function effectively defines the policy of this scheduler.
    fn select_next_task(&self, rq_id: Self::RunqueueId) -> Option<TaskRef>;

    /// Iterates over all runqueues to remove the given task from each one.
    fn remove_task_from_all_runqueues(&self, task: &TaskRef) -> Result<(), RunqueueError>;

    /// Returns the requested runqueue.
    fn get_runqueue(&self, rq_id: Self::RunqueueId) -> Option<&'static RwLockPreempt<Self::Runqueue>>;

    /// Returns the "least busy" runqueue, currently based only on runqueue size.
    fn get_least_busy_runqueue(&self) -> Option<&'static RwLockPreempt<Self::Runqueue>>;
    
    /// Adds the given task to the "least busy" runqueue.
    fn add_task_to_any_runqueue(&self, task: Self::RunqueueTaskRef) -> Result<(), RunqueueError>;

    /// Adds the given task to the given runqueue.
    fn add_task_to_specific_runqueue(&self, rq_id: Self::RunqueueId, task: Self::RunqueueTaskRef) -> Result<(), RunqueueError>;
}

/// The set of errors that may occur in [`SchedulerPolicy`] functions.
#[derive(Debug)]
pub enum RunqueueError {
    /// A runqueue already existed for the 
    RunqueueAlreadyExists,
    /// When trying to get access a specific runqueue, that runqueue could not be found.
    RunqueueNotFound,
    /// When trying to add a task to a runqueue, that task was already present.
    TaskAlreadyExists,
    /// When trying to remove a task from a runqueue, that task was not found.
    TaskNotFound,
}

/// A dummy scheduler policy that does nothing and returns errors for all funcitons.
pub struct DummyScheduler;
impl SchedulerPolicy for DummyScheduler {
    type Runqueue = ();
    type RunqueueId = ();
    type RunqueueTaskRef = TaskRef;

    fn init_runqueue(&self, _rq_id: Self::RunqueueId) -> Result<(), RunqueueError> {
        Err(RunqueueError::RunqueueAlreadyExists)
    }
    fn select_next_task(&self, _rq_id: Self::RunqueueId) -> Option<TaskRef> {
        None
    }
    fn remove_task_from_all_runqueues(&self, _task: &TaskRef) -> Result<(), RunqueueError> {
        Err(RunqueueError::TaskNotFound)
    }
    fn get_runqueue(&self, _rq_id: Self::RunqueueId) -> Option<&'static RwLockPreempt<Self::Runqueue>> {
        None
    }
    fn get_least_busy_runqueue(&self) -> Option<&'static RwLockPreempt<Self::Runqueue>> {
        None
    }
    fn add_task_to_any_runqueue(&self, _task: Self::RunqueueTaskRef) -> Result<(), RunqueueError> {
        Err(RunqueueError::RunqueueNotFound)
    }
    fn add_task_to_specific_runqueue(&self, _rq_id: Self::RunqueueId, _task: Self::RunqueueTaskRef) -> Result<(), RunqueueError> {
        Err(RunqueueError::RunqueueNotFound)
    }
}
