use crate::{Error, Result, Shell};
use alloc::{borrow::ToOwned, vec::Vec};
use app_io::println;
use path::Path;

// TODO: Decide which internal commands we don't need.

impl Shell {
    pub(crate) fn alias(&self, args: Vec<&str>) -> Result<()> {
        todo!();
    }

    pub(crate) fn bg(&mut self, args: Vec<&str>) -> Result<()> {
        if args.is_empty() {
            if let Some(num) = self.stop_order.pop() {
                let task = self.jobs.get(&num).unwrap();
                task.unblock();
                // TODO: Print
                Ok(())
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
            Ok(())
        }
    }

    pub(crate) fn cd(&self, args: Vec<&str>) -> Result<()> {
        if args.len() > 1 {
            return Err(Error::Command(1));
        }

        let path = Path::new(if let Some(arg) = args.get(0) {
            (*arg).to_owned()
        } else {
            "/".to_owned()
        });

        let task = task::get_my_current_task().unwrap();

        match task.get_env().lock().chdir(&path) {
            Ok(()) => Ok(()),
            Err(_) => todo!(),
        }
    }

    pub(crate) fn exec(&self, args: Vec<&str>) -> Result<()> {
        todo!();
    }

    pub(crate) fn exit(&self, args: Vec<&str>) -> Result<()> {
        // TODO: Clean up background tasks?
        Err(Error::ExitRequested)
    }

    pub(crate) fn export(&self, args: Vec<&str>) -> Result<()> {
        todo!();
    }

    pub(crate) fn fc(&self, args: Vec<&str>) -> Result<()> {
        todo!();
    }

    pub(crate) fn fg(&self, args: Vec<&str>) -> Result<()> {
        todo!();
    }

    pub(crate) fn getopts(&self, args: Vec<&str>) -> Result<()> {
        todo!();
    }

    pub(crate) fn hash(&self, args: Vec<&str>) -> Result<()> {
        todo!();
    }

    pub(crate) fn history(&self, args: Vec<&str>) -> Result<()> {
        todo!();
    }

    pub(crate) fn jobs(&self, args: Vec<&str>) -> Result<()> {
        todo!();
    }

    pub(crate) fn set(&self, args: Vec<&str>) -> Result<()> {
        todo!();
    }

    pub(crate) fn unalias(&self, args: Vec<&str>) -> Result<()> {
        todo!();
    }

    pub(crate) fn unset(&self, args: Vec<&str>) -> Result<()> {
        todo!();
    }

    pub(crate) fn wait(&self, args: Vec<&str>) -> Result<()> {
        todo!();
    }
}
