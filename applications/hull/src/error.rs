//! Shell-specific errors.

use alloc::string::String;
use task::RunState;

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Clone, Debug)]
pub enum Error {
    /// The user requested the shell to exit.
    ExitRequested,
    /// The shell could not access its task struct.
    CurrentTaskUnavailable,
    /// The command input by the user could not be resolved.
    ///
    /// The input command is stored in the field.
    CommandNotFound(String),
    /// The command returned with a non-zero exit code.
    ///
    /// The exit code is stored in the field.
    Command(isize),
    /// Failed to kill a task.
    KillFailed,
    /// Failed to spawn a task.
    SpawnFailed,
    /// Failed to unblock a task.
    ///
    /// The current runstate is stored in the field.
    UnblockFailed(RunState),
}
