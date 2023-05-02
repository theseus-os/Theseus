//! Defines the [`SchedulerPolicy`] trait, an abstraction of scheduler policies.

#![no_std]
#![feature(trait_alias)]

extern crate alloc;

use atomic_linked_list::atomic_map::AtomicMapIter;
use mutex_preemption::RwLockPreempt;
use runqueue_round_robin::RunqueueRoundRobin;
use task::TaskRef;

pub use runqueue_trait::*;


/// An abstraction of a scheduler policy.
///
/// A scheduler policy can be registered using [`task::set_scheduler_policy()`]
/// and will be used by future invocations of the `schedule()` routine.
pub trait SchedulerPolicy {
    /// Initializes a new runqueue with the given runqueue RunqueueId in this scheduler.
    ///
    /// Returns [`RunqueueError::RunqueueAlreadyExists`] if a runqueue with the
    /// given `rq_id` already exists in this scheduler.
    fn init_runqueue(&self, rq_id: RunqueueId) -> Result<(), RunqueueError>;

    /// Returns the next task that should be scheduled in,
    /// or `None` if there are no runnable tasks.
    ///
    /// This function effectively defines the policy of this scheduler.
    fn select_next_task(&self, rq_id: RunqueueId) -> Option<TaskRef>;

    /// Adds the given task to an optionally-specified runqueue in this scheduler.
    ///
    /// If `rq_id` is `None`, this scheduler will add the given task
    /// to a runqueue of its choice, e.g., the "least busy" one.
    ///
    /// Returns [`RunqueueError::RunqueueAlreadyExists`] if a runqueue with the
    /// given `rq_id` already exists in this scheduler.
    fn add_task(&self, task: TaskRef, rq_id: Option<RunqueueId>) -> Result<(), RunqueueError>;

    /// Removes the given task from this scheduler's runqueue(s).
    fn remove_task(&self, task: &TaskRef) -> Result<(), RunqueueError>;

    /// Returns an iterator over all runqueues in this scheduler.
    fn runqueue_iter(&self) -> AllRunqueuesIterator;

    /// Returns a reference to the runqueue with the given `RunqueueId`.
    ///
    /// This is a provided method that iterates over all runqueues until
    /// it finds one with the matching runqueue ID.
    /// However, each scheduler may re-implement it more efficiently, e.g.,
    /// via a direct lookup on runqueues stored in a map-like structure.
    fn get_runqueue(&self, rq_id: RunqueueId) -> Option<RunqueueRef> {
        self.runqueue_iter()
            .find(|r| r.id() == rq_id)
            .map(Into::into)
    }
}

/// A dummy scheduler policy that does nothing and returns errors for all functions.
pub struct DummyScheduler;
impl DummyScheduler {
    pub const fn new() -> Self {
        Self
    }
}
impl SchedulerPolicy for DummyScheduler {
    fn init_runqueue(&self, _: RunqueueId) -> Result<(), RunqueueError> {
        Err(RunqueueError::RunqueueAlreadyExists)
    }
    fn select_next_task(&self, _: RunqueueId) -> Option<TaskRef> {
        None
    }
    fn add_task(&self, _task: TaskRef, _: Option<RunqueueId>) -> Result<(), RunqueueError> {
        Err(RunqueueError::RunqueueNotFound)            
    }
    fn remove_task(&self, _task: &TaskRef) -> Result<(), RunqueueError> {
        Err(RunqueueError::TaskNotFound)
    }
    fn runqueue_iter(&self) -> AllRunqueuesIterator {
        todo!()
        // AllRunqueuesIterator(&mut core::iter::empty::<&RunqueueRef>()) // { id: RunqueueId(0), _phantom: PhantomData }
    }
}

/// An iterator over all runqueues in a given scheduler.
pub struct AllRunqueuesIterator<'a>(AtomicMapIter<'a, RunqueueId, RwLockPreempt<RunqueueRoundRobin>>);
impl<'a> From<AtomicMapIter<'a, RunqueueId, RwLockPreempt<RunqueueRoundRobin>>> for AllRunqueuesIterator<'a> {
    fn from(iter: AtomicMapIter<'a, RunqueueId, RwLockPreempt<RunqueueRoundRobin>>) -> Self {
        Self(iter)
    }
}
impl<'a> Iterator for AllRunqueuesIterator<'a> {
    type Item = &'a RwLockPreempt<RunqueueRoundRobin>;
    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(|(_id, r)| r)
    }
}

// /// An iterator over all runqueues in a given scheduler.
// pub struct AllRunqueuesIterator<'a, R, I>
// where
//     R: RunqueueTrait,
//     I: Iterator<Item = &'a R>,
// {
//     iter: I,
// }

// impl<'a, R, I> From<I> for AllRunqueuesIterator<'a, R, I>
// where
//     R: RunqueueTrait,
//     I: Iterator<Item = &'a R>,
// {
//     fn from(iter: I) -> Self {
//         Self { iter }
//     }
// }
// impl<'a, R, I> Iterator for AllRunqueuesIterator<'a, R, I> {
//     type Item = &'a R;
//     fn next(&mut self) -> Option<Self::Item> {
//         self.0.next()
//     }
// }


////////////////////////////////////////////

