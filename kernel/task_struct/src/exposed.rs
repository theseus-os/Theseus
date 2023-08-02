//! Defines an exposed task reference.

use core::{ops::Deref, sync::atomic::AtomicBool};

use cpu::OptionalCpuId;
use crossbeam_utils::atomic::AtomicCell;
use mod_mgmt::TlsDataImage;
use spin::Mutex;
use sync_irq::IrqSafeMutex;

use crate::{ExitValue, RawTaskRef, RunState, Task, TaskInner};

/// A type wrapper that exposes public access to all inner fields of a task.
///
/// This is intended for use by the `task` crate, specifically within a
/// `TaskRef`. This can only be obtained by consuming a fully-initialized
/// [`Task`], which makes it completely safe to be public because there is
/// nowhere else besides within the `TaskRef::create()` constructor that one can
/// obtain access to an owned `Task` value that is already registered/spawned
/// (actually usable).
///
/// If another crate instantiates a bare `RawTaskRef` (not a `Taskref`) and then
/// converts it into this `ExposedTask` type, then there's nothing they can do
/// with that task because it cannot become a spawnable/schedulable/runnable
/// task until it is passed into `TaskRef::create()`, so that'd be completely
/// harmless.
#[doc(hidden)]
#[derive(Clone)]
pub struct ExposedTaskRef<'a> {
    pub inner: &'a RawTaskRef,
}

impl<'a> Deref for ExposedTaskRef<'a> {
    type Target = Task;

    fn deref(&self) -> &Self::Target {
        self.inner
    }
}

// Here we simply expose accessors for all private fields of `Task`.
impl<'a> ExposedTaskRef<'a> {
    #[inline(always)]
    pub fn inner(&self) -> &IrqSafeMutex<TaskInner> {
        &self.inner.inner.inner
    }

    #[inline(always)]
    pub fn tls_area(&self) -> &TlsDataImage {
        &self.tls_area
    }

    #[inline(always)]
    pub fn running_on_cpu(&self) -> &AtomicCell<OptionalCpuId> {
        &self.running_on_cpu
    }

    #[inline(always)]
    pub fn runstate(&self) -> &AtomicCell<RunState> {
        &self.runstate
    }

    #[inline(always)]
    pub fn is_on_run_queue(&self) -> &AtomicBool {
        &self.is_on_run_queue
    }

    #[inline(always)]
    pub fn joinable(&self) -> &AtomicBool {
        &self.joinable
    }

    #[inline(always)]
    pub fn exit_value_mailbox(&self) -> &Mutex<Option<ExitValue>> {
        &self.exit_value_mailbox
    }
}
