//! Shell-specific errors.

use alloc::string::String;
use app_io::println;
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
    SpawnFailed(&'static str),
    /// Failed to unblock a task.
    ///
    /// The current runstate is stored in the field.
    UnblockFailed(RunState),
}

impl Error {
    /// Prints this error if it is recoverable, otherwise returning the error.
    pub(crate) fn print(self) -> Result<()> {
        match self {
            Error::ExitRequested => return Err(Error::ExitRequested),
            Error::CurrentTaskUnavailable => return Err(Error::CurrentTaskUnavailable),
            Error::Command(exit_code) => println!("exit {}", exit_code),
            Error::CommandNotFound(command) => println!("{}: command not found", command),
            Error::SpawnFailed(s) => println!("failed to spawn task: {s}"),
            Error::KillFailed => println!("failed to kill task"),
            Error::UnblockFailed(state) => {
                println!("failed to unblock task with state {:?}", state)
            }
        }
        Ok(())
    }
}
