use crate::{Error, Result, Shell};
use alloc::vec::Vec;

// TODO: Decide which internal commands we don't need.

impl Shell {
    pub(crate) fn alias(&self, args: Vec<&str>) -> Result<isize> {
        todo!();
    }

    pub(crate) fn bg(&mut self, args: Vec<&str>) -> Result<isize> {
        if args.is_empty() {
            if let Some(num) = self.stop_order.pop() {
                let task = self.jobs.get(&num).unwrap();
                task.unblock();
                // TODO: Print
                Ok(0)
            } else {
                todo!("no current job");
            }
        } else {
            for arg in args {
                if !arg.starts_with('%') {
                    todo!("job not found: {arg}");
                } else {
                    let num = arg[1..].parse().unwrap();
                    let task = self.jobs.get(&num).unwrap();
                    task.unblock();
                    // TODO: Print
                    continue;
                }
            }
            Ok(0)
        }
    }

    pub(crate) fn cd(&self, args: Vec<&str>) -> Result<isize> {
        todo!();
    }

    pub(crate) fn exec(&self, args: Vec<&str>) -> Result<isize> {
        todo!();
    }

    pub(crate) fn exit(&self, args: Vec<&str>) -> Result<isize> {
        // TODO: Clean up background tasks?
        Err(Error::Exit)
    }

    pub(crate) fn export(&self, args: Vec<&str>) -> Result<isize> {
        todo!();
    }

    pub(crate) fn fc(&self, args: Vec<&str>) -> Result<isize> {
        todo!();
    }

    pub(crate) fn fg(&self, args: Vec<&str>) -> Result<isize> {
        todo!();
    }

    pub(crate) fn getopts(&self, args: Vec<&str>) -> Result<isize> {
        todo!();
    }

    pub(crate) fn hash(&self, args: Vec<&str>) -> Result<isize> {
        todo!();
    }

    pub(crate) fn history(&self, args: Vec<&str>) -> Result<isize> {
        todo!();
    }

    pub(crate) fn jobs(&self, args: Vec<&str>) -> Result<isize> {
        todo!();
    }

    pub(crate) fn set(&self, args: Vec<&str>) -> Result<isize> {
        todo!();
    }

    pub(crate) fn unalias(&self, args: Vec<&str>) -> Result<isize> {
        todo!();
    }

    pub(crate) fn unset(&self, args: Vec<&str>) -> Result<isize> {
        todo!();
    }

    pub(crate) fn wait(&self, args: Vec<&str>) -> Result<isize> {
        todo!();
    }
}
