//! This crate contains the basic [`Task`] structure, which holds contextual execution states
//! needed to support safe multithreading.
//! 
//! To create new `Task`s, use the [`spawn`](../spawn/index.html) crate.
//! For more advanced task-related types, see the [`task`](../task/index.html) crate.

#![no_std]
#![feature(panic_info_message)]
#![feature(negative_impls)]
#![allow(clippy::type_complexity)]

extern crate alloc;

mod exitable;
mod exposed;
mod inherited_states;
mod rref;
mod task;

use alloc::{boxed::Box, collections::BTreeMap, format, string::String};
use core::{any::Any, fmt, panic::PanicInfo};

pub use exitable::ExitableTaskRef;
pub use exposed::ExposedTaskRef;
pub use inherited_states::InheritedStates;
pub use rref::{RawTaskRef, RawWeakTaskRef};
use sync_irq::IrqSafeMutex;
pub use task::{Task, TaskInner};

/// The list of all Tasks in the system.
#[doc(hidden)]
pub static TASK_LIST: IrqSafeMutex<BTreeMap<usize, RawTaskRef>> = IrqSafeMutex::new(BTreeMap::new());

/// The function signature of the callback that will be invoked when a `Task`
/// panics or otherwise fails, e.g., a machine exception occurs.
pub type KillHandler = Box<dyn Fn(&KillReason) + Send>;

/// The signature of a Task's failure cleanup function.
pub type FailureCleanupFunction = fn(ExitableTaskRef, KillReason) -> !;

/// Just like `core::panic::PanicInfo`, but with owned String types instead of &str references.
#[derive(Debug, Default)]
pub struct PanicInfoOwned {
    pub payload:  Option<Box<dyn Any + Send>>,
    pub msg:      String,
    pub file:     String,
    pub line:     u32, 
    pub column:   u32,
}
impl fmt::Display for PanicInfoOwned {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(f, "{}:{}:{} -- {:?}", self.file, self.line, self.column, self.msg)
    }
}
impl<'p> From<&PanicInfo<'p>> for PanicInfoOwned {
    fn from(info: &PanicInfo) -> PanicInfoOwned {
        let msg = info.message()
            .map(|m| format!("{m}"))
            .unwrap_or_default();
        let (file, line, column) = if let Some(loc) = info.location() {
            (String::from(loc.file()), loc.line(), loc.column())
        } else {
            (String::new(), 0, 0)
        };

        PanicInfoOwned { payload: None, msg, file, line, column }
    }
}
impl PanicInfoOwned {
    /// Constructs a new `PanicInfoOwned` object containing only the given `payload`
    /// without any location or message info.
    /// 
    /// Useful for forwarding panic payloads through a catch and resume unwinding sequence.
    pub fn from_payload(payload: Box<dyn Any + Send>) -> PanicInfoOwned {
        PanicInfoOwned {
            payload: Some(payload),
            ..Default::default()
        }
    }
}


/// The list of possible reasons that a given `Task` was killed prematurely.
#[derive(Debug)]
pub enum KillReason {
    /// The user or another task requested that this `Task` be killed. 
    /// For example, the user pressed `Ctrl + C` on the shell window that started a `Task`.
    Requested,
    /// A Rust-level panic occurred while running this `Task`.
    Panic(PanicInfoOwned),
    /// A non-language-level problem, such as a Page Fault or some other machine exception.
    /// The number of the exception is included, e.g., 15 (0xE) for a Page Fault.
    Exception(u8),
}
impl fmt::Display for KillReason {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        match self {
            Self::Requested         => write!(f, "Requested"),
            Self::Panic(panic_info) => write!(f, "Panicked at {panic_info}"),
            Self::Exception(num)    => write!(f, "Exception {num:#X}({num})"),
        }
    }
}


/// The two ways a `Task` can exit, including possible return values and conditions.
#[derive(Debug)]
pub enum ExitValue {
    /// The `Task` ran to completion
    /// and returned the enclosed [`Any`] value from its entry point function.
    ///
    /// The caller of this task's entry point function should know which concrete type
    /// this Task returned, and is thus able to downcast it appropriately.
    Completed(Box<dyn Any + Send>),
    /// The `Task` did NOT run to completion but was instead killed for the enclosed reason.
    Killed(KillReason),
}


/// The set of possible runstates that a `Task` can be in.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RunState {
    /// This task is in the midst of being initialized/spawned.
    Initing,
    /// This task is able to be scheduled in, but not necessarily currently running.
    /// To check whether it is currently running, use [`Task::is_running()`].
    Runnable,
    /// This task is blocked on something and is *not* able to be scheduled in.
    Blocked,
    /// This `Task` has exited and can no longer be run.
    /// This covers both the case when a task ran to completion or was killed;
    /// see [`ExitValue`] for more details.
    Exited,
    /// This `Task` had already exited, and now its [`ExitValue`] has been taken
    /// (either by another task that `join`ed it, or by the system).
    /// Because a task's exit value can only be taken once, a repaed task
    /// is useless and will be cleaned up and removed from the system.
    Reaped,
}


#[cfg(simd_personality)]
/// The supported levels of SIMD extensions that a `Task` can use.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum SimdExt {
    /// AVX (and below) instructions and registers will be used.
    AVX,
    /// SSE instructions and registers will be used.
    SSE,
    /// The regular case: no SIMD instructions or registers of any kind will be used.
    None,
}

/// A struct holding data items needed to restart a `Task`.
pub struct RestartInfo {
    /// Stores the argument of the task for restartable tasks
    pub argument: Box<dyn Any + Send>,
    /// Stores the function of the task for restartable tasks
    pub func: Box<dyn Any + Send>,
}
