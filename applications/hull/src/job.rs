//! Shell job control.

use crate::{Error, Result};
use alloc::vec::Vec;
use task::{ExitValue, JoinableTaskRef, KillReason, RunState};

/// A shell job consisting of multiple parts.
///
/// E.g. `sleep 5 | sleep 10` is one job consisting of two job parts.
#[derive(Debug)]
pub(crate) struct Job {
    pub(crate) parts: Vec<JobPart>,
}

impl Job {
    pub(crate) fn kill(&mut self) -> Result<()> {
        for mut part in self.parts.iter_mut() {
            part.task
                .kill(KillReason::Requested)
                .map_err(|_| Error::KillFailed)?;
            part.state = State::Complete(130);
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
                part.state = State::Complete(exit_value);
            }
        }
        self.exit_value()
    }

    pub(crate) fn exit_value(&mut self) -> Option<isize> {
        if let State::Complete(value) = self.parts.last()?.state {
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
    Complete(isize),
    Suspended,
    Running,
}

impl core::cmp::PartialEq for State {
    fn eq(&self, other: &Self) -> bool {
        matches!(
            (self, other),
            (State::Complete(_), State::Complete(_))
                | (State::Suspended, State::Suspended)
                | (State::Running, State::Running)
        )
    }
}

impl core::cmp::Eq for State {}
