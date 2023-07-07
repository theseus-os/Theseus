//! Shell job control.

use core::fmt;

use crate::{Error, Result};
use alloc::{string::String, vec::Vec};
use task::{KillReason, TaskRef};

/// A shell job consisting of multiple parts.
///
/// E.g. `sleep 5 | sleep 10` is one job consisting of two job parts.
///
/// Backgrounded tasks (e.g. `sleep 1` in `sleep 1 & sleep 2`) are a separate
/// job.
#[derive(Debug, Default)]
pub(crate) struct Job {
    pub(crate) string: String,
    pub(crate) parts: Vec<JobPart>,
    pub(crate) current: bool,
}

impl Job {
    pub(crate) fn kill(&mut self) -> Result<()> {
        for part in self.parts.iter_mut() {
            part.task
                .kill(KillReason::Requested)
                .map_err(|_| Error::KillFailed)?;
            part.state = State::Done(130);
        }
        Ok(())
    }
    #[allow(unused)]
    pub(crate) fn suspend(&mut self) {
        for part in self.parts.iter_mut() {
            part.task.suspend();
            part.state = State::Suspended;
        }
    }

    pub(crate) fn unsuspend(&mut self) {
        for part in self.parts.iter_mut() {
            part.task.unsuspend();
            part.state = State::Running;
        }
    }

    pub(crate) fn exit_value(&mut self) -> Option<isize> {
        if self
            .parts
            .iter()
            .all(|part| matches!(part.state, State::Done(_)))
        {
            if let State::Done(value) = self.parts.last()?.state {
                Some(value)
            } else {
                unreachable!("tried to get exit value of empty job: {self:?}");
            }
        } else {
            None
        }
    }
}

#[derive(Debug)]
pub(crate) struct JobPart {
    pub(crate) state: State,
    pub(crate) task: TaskRef,
}

#[derive(Debug)]
pub(crate) enum State {
    Done(isize),
    #[allow(unused)]
    Suspended,
    Running,
}

impl fmt::Display for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Done(_) => "done",
            Self::Suspended => "suspended",
            Self::Running => "running",
        })
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
