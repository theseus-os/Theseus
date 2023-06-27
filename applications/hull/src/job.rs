//! Shell job control.

use core::fmt;

use crate::{Error, Result};
use alloc::{string::String, vec::Vec};
use task::{ExitValue, JoinableTaskRef, KillReason, RunState};

/// A shell job consisting of multiple parts.
///
/// E.g. `sleep 5 | sleep 10` is one job consisting of two job parts.
///
/// Backgrounded tasks (e.g. `sleep 1` in `sleep 1 & sleep 2`) are a separate
/// job.
#[derive(Debug, Default)]
pub(crate) struct Job {
    pub(crate) line: String,
    pub(crate) parts: Vec<JobPart>,
}

impl Job {
    pub(crate) fn kill(&mut self) -> Result<()> {
        for mut part in self.parts.iter_mut() {
            part.task
                .kill(KillReason::Requested)
                .map_err(|_| Error::KillFailed)?;
            part.state = State::Done(130);
        }
        Ok(())
    }
    pub(crate) fn suspend(&mut self) {
        for mut part in self.parts.iter_mut() {
            part.task.suspend();
            part.state = State::Suspended;
        }
    }

    pub(crate) fn unsuspend(&mut self) {
        for mut part in self.parts.iter_mut() {
            part.task.unsuspend();
            part.state = State::Running;
        }
    }

    pub(crate) fn unblock(&mut self) -> Result<()> {
        for mut part in self.parts.iter_mut() {
            part.task.unblock().map_err(Error::UnblockFailed)?;
            part.state = State::Running;
        }
        Ok(())
    }

    pub(crate) fn update(&mut self) -> Option<isize> {
        for mut part in self.parts.iter_mut() {
            if part.state == State::Running && part.task.runstate() == RunState::Exited {
                let exit_value = match part.task.join().unwrap() {
                    ExitValue::Completed(status) => {
                        match status.downcast_ref::<isize>() {
                            Some(num) => *num,
                            // FIXME: Document/decide on a number for when app doesn't
                            // return isize.
                            None => 210,
                        }
                    }
                    ExitValue::Killed(reason) => match reason {
                        // FIXME: Document/decide on a number. This is used by bash.
                        KillReason::Requested => 130,
                        KillReason::Panic(_) => 1,
                        KillReason::Exception(num) => num.into(),
                    },
                };
                part.state = State::Done(exit_value);
            }
        }
        self.exit_value()
    }

    pub(crate) fn exit_value(&mut self) -> Option<isize> {
        if let State::Done(value) = self.parts.last()?.state {
            Some(value)
        } else {
            None
        }
    }
}

#[derive(Debug)]
pub(crate) struct JobPart {
    pub(crate) state: State,
    pub(crate) task: JoinableTaskRef,
}

#[derive(Debug)]
pub(crate) enum State {
    Done(isize),
    Suspended,
    Running,
}

impl fmt::Display for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Done(_) => write!(f, "done"),
            Self::Suspended => write!(f, "suspended"),
            Self::Running => write!(f, "running"),
        }
    }
}

impl core::cmp::PartialEq for State {
    fn eq(&self, other: &Self) -> bool {
        matches!(
            (self, other),
            (State::Done(_), State::Done(_))
                | (State::Suspended, State::Suspended)
                | (State::Running, State::Running)
        )
    }
}

impl core::cmp::Eq for State {}
