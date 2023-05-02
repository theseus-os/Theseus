//! Defines the [`RunqueueTrait`] trait, an abstraction of a scheduler runqueue.

#![no_std]
#![feature(trait_alias)]
#![feature(type_alias_impl_trait)]

extern crate alloc;

use alloc::vec::Vec;
use task::TaskRef;

/// A unique identifier for a runqueue in a scheduler policy implementation.
///
/// This trait is implemented for any blanket type that implements `PartialEq`.
pub trait SchedulerRunqueueId: PartialEq { }
impl<P: PartialEq> SchedulerRunqueueId for P { }


/// A unique identifier for a runqueue in a scheduler.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct RunqueueId(u64);
impl<T: Into<u64>> From<T> for RunqueueId {
    fn from(value: T) -> Self {
        Self(value.into())
    }
}

/// A reference to a runqueue in a given scheduler.
pub struct RunqueueRef<'a>(&'a dyn RunqueueTrait);
impl<'a, R: RunqueueTrait> From<&'a R> for RunqueueRef<'a> {
    fn from(value: &'a R) -> Self {
        Self(value)
    }
}
impl<'a> RunqueueTrait for RunqueueRef<'a> {
    fn len(&self) -> usize {
        self.0.len()
    }
    fn id(&self) -> RunqueueId {
        self.0.id()
    }
    fn task_iter(&self) -> TaskIter {
        self.0.task_iter()
    }
}


/// An abstraction of a runqueue, which holds schedulable tasks.
pub trait RunqueueTrait {
    /// Returns the unique ID of this runqueue.
    fn id(&self) -> RunqueueId;

    /// Returns the number of tasks currently in this runqueue.
    ///
    /// This returns `0` ff the underlying implementation
    /// does not support tracking its length (e.g., a FIFO queue).
    fn len(&self) -> usize;

    /// Returns an iterator over all tasks currently on this runqueue.
    ///
    /// This returns an empty iterator if the underlying implementation
    /// does not support iteration (e.g., a FIFO queue).
    // fn task_iter<'a, I: Iterator<Item = &'a TaskRef>>(&'a self) -> TaskIter<'a, I>;
    fn task_iter(&self) -> TaskIter;
}
impl<R: RunqueueTrait> RunqueueTrait for mutex_preemption::RwLockPreempt<R> {
    fn id(&self) -> RunqueueId { self.read().id() }
    fn len(&self) -> usize { self.read().len() }
    fn task_iter(&self) -> TaskIter { self.read().task_iter() }
}

/// An iterator over all tasks in a runqueue.
pub struct TaskIter(Vec<TaskRef>);
impl From<Vec<TaskRef>> for TaskIter {
    fn from(list: Vec<TaskRef>) -> Self {
        Self(list)
    }
}
impl<'a> Iterator for TaskIter {
    type Item = TaskRef;
    fn next(&mut self) -> Option<Self::Item> {
        self.0.pop()
    }
}

// pub struct TaskIter<'a, I: Iterator<Item = &'a TaskRef>>(I);
// impl<'a, I: Iterator<Item = &'a TaskRef>> Iterator for TaskIter<'a, I> {
//     type Item = &'a TaskRef;
//     fn next(&mut self) -> Option<Self::Item> {
//         self.0.next()
//     }
// }

/// The set of errors that may occur when modifying or accessing scheduler runqueues.
#[derive(Clone, Copy, Debug)]
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
impl RunqueueError {
    pub fn into_static_str(self) -> &'static str {
        self.into()
    }
}
impl From<RunqueueError> for &'static str {
    fn from(err: RunqueueError) -> Self {
        match err {
            RunqueueError::RunqueueAlreadyExists => "runqueue already exists in scheduler",
            RunqueueError::RunqueueNotFound => "runqueue not found in scheduler",
            RunqueueError::TaskAlreadyExists => "task already exists on runqueue",
            RunqueueError::TaskNotFound => "task not found on runqueue"
        }
    }
}



///////////////////////////////////////////////////////////////////////////////////////////
//////////////////  Attempt at an erased object-safe trait ////////////////////////////////
///////////////////////////////////////////////////////////////////////////////////////////

// Notes:
// * Based on: https://github.com/dtolnay/erased-serde/blob/master/explanation/main.rs
// * Further discussed at https://www.possiblerust.com/pattern/3-things-to-try-when-you-can-t-make-a-trait-object
// * https://stackoverflow.com/questions/74806682/how-to-make-type-erased-version-of-a-trait-with-associated-type
// * https://users.rust-lang.org/t/help-for-syntax-for-constraining-the-associated-type/51766
// * This proc_macro crate might work: https://docs.rs/dynamize/latest/dynamize/


/// Analogous to `Querializer`  in the erased_serde example
pub trait SimpleRunqueueTrait {
    /// Returns the unique ID of this runqueue.
    fn id(&self) -> RunqueueId;

    /// Returns the number of tasks currently in this runqueue.
    ///
    /// This returns `0` ff the underlying implementation
    /// does not support tracking its length (e.g., a FIFO queue).
    fn len(&self) -> usize;
}
impl<R: SimpleRunqueueTrait + ?Sized> SimpleRunqueueTrait for mutex_preemption::RwLockPreempt<R> {
    fn id(&self) -> RunqueueId { self.read().id() }
    fn len(&self) -> usize { self.read().len() }
}
impl<'a, T: ?Sized> SimpleRunqueueTrait for &'a T where T: SimpleRunqueueTrait {
    fn id(&self) -> RunqueueId { (**self).id() }
    fn len(&self) -> usize { (**self).len() }
}


/// Analogous to "Generic" in the erased_serde example
pub trait GenericSchedulerPolicy<'a> {
    type RunqueueType: SimpleRunqueueTrait + ?Sized + 'a;
    
    /// Returns a reference to the runqueue with the given `RunqueueId`.
    fn with_runqueue(&'a self, rq_id: RunqueueId) -> Option<&'a Self::RunqueueType>;
}


/// Analogous to `ErasedGeneric`  in the erased_serde example
pub trait ErasedGenericSchedulerPolicy<'a> {
    fn erased_with_runqueue(&'a self, rq_id: RunqueueId) -> Option<&'a (dyn SimpleRunqueueTrait + 'a)>;
}

impl<'a> GenericSchedulerPolicy<'a> for dyn ErasedGenericSchedulerPolicy<'a> {
    type RunqueueType = dyn SimpleRunqueueTrait + 'a;
    fn with_runqueue(&'a self, rq_id: RunqueueId) -> Option<&'a Self::RunqueueType> {
        self.erased_with_runqueue(rq_id)
    }
}

// impl<'a, EGST> GenericSchedulerPolicy<'a> for EGST
// where
//     EGST: ErasedGenericSchedulerPolicy<'a> + 'a,
// {
//     type RunqueueType = dyn UnsizedSimpleRunqueueTrait + 'a;
//     fn with_runqueue(&'a self, rq_id: RunqueueId) -> Option<&'a Self::RunqueueType> {
//         self.erased_with_runqueue(rq_id)
//     }
// }

impl<'a, T> ErasedGenericSchedulerPolicy<'a> for T
where
    T: GenericSchedulerPolicy<'a>,
    // <T as GenericSchedulerPolicy<'a>>::RunqueueType: (dyn SimpleRunqueueTrait + 'a),
    T: GenericSchedulerPolicy<'a, RunqueueType = dyn SimpleRunqueueTrait + 'a>       // works, but incompatible with type RunqueueType = RwLockPreempt<RunqueueRoundRobin>
    // T: GenericSchedulerPolicy<'a, RunqueueType = R>,
    // <T as GenericSchedulerPolicy<'a>>::RunqueueType: dyn SimpleRunqueueTrait,
{
    fn erased_with_runqueue(&'a self, rq_id: RunqueueId) -> Option<&'a (dyn SimpleRunqueueTrait + 'a)> {
        self.with_runqueue(rq_id)
            // .map(|v| &v as &dyn SimpleRunqueueTrait)
    }
}
