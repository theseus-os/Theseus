use alloc::sync::{Arc, Weak};
use core::ops::Deref;

use crate::{ExposedTaskRef, Task};

/// An atomically reference counted task.
///
/// This type solves the circular dependency between the scheduler and task
/// subsystems. The `TaskRef` struct has methods which interact with the
/// scheduler subsystem (e.g. `block` and `unblock`). However, this creates a
/// circular dependency as the scheduler subsystem nedes to store task
/// references on the run queue. To circumvent this, the scheduler stores
/// `RawTaskRef`s on the run queue which don't depend on the scheduler
/// subsystem, but also doesn't expose any methods that depend on the scheduler
/// subsystem.
#[derive(Debug, Clone)]
pub struct RawTaskRef {
    pub inner: Arc<Task>,
}

impl PartialEq for RawTaskRef {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl Eq for RawTaskRef {}

impl Deref for RawTaskRef {
    type Target = Arc<Task>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl RawTaskRef {
    pub fn downgrade(this: &Self) -> RawWeakTaskRef {
        RawWeakTaskRef {
            inner: Arc::downgrade(&this.inner),
        }
    }

    #[doc(hidden)]
    pub fn expose(&self) -> ExposedTaskRef<'_> {
        ExposedTaskRef { inner: &self }
    }
}

#[derive(Debug, Clone)]
pub struct RawWeakTaskRef {
    pub inner: Weak<Task>,
}

impl RawWeakTaskRef {
    pub fn upgrade(&self) -> Option<RawTaskRef> {
        self.inner.upgrade().map(|inner| RawTaskRef { inner })
    }
}
