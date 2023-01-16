//! Builtin shell commands.

use crate::{Error, Result, Shell};
use alloc::{borrow::ToOwned, vec::Vec};
use app_io::println;
use path::Path;

// TODO: Decide which builtins we don't need.

impl Shell {
    pub(crate) fn alias(&self, _args: Vec<&str>) -> Result<()> {
        println!("not yet implemented");
        Err(Error::Command(1))
    }

    pub(crate) fn bg(&mut self, args: Vec<&str>) -> Result<()> {
        if args.is_empty() {
            loop {
                let num = match self.stop_order.pop() {
                    Some(n) => n,
                    None => {
                        println!("no current job");
                        return Err(Error::Command(1));
                    }
                };
                if let Some(task) = self.jobs.get_mut(&num) {
                    task.unsuspend();
                    // TODO: Print
                    return Ok(());
                }
            }
        } else {
            for arg in args {
                if !arg.starts_with('%') {
                    println!("job not found: {}", arg);
                    return Err(Error::Command(1));
                } else {
                    let num = match arg[1..].parse() {
                        Ok(n) => n,
                        Err(_) => {
                            println!("job not found: {}", &arg[1..]);
                            return Err(Error::Command(1));
                        }
                    };
                    let task = match self.jobs.get_mut(&num) {
                        Some(t) => t,
                        None => {
                            println!("{}: no such job", arg);
                            return Err(Error::Command(1));
                        }
                    };
                    task.unsuspend();
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

        let path = Path::new(if let Some(arg) = args.first() {
            (*arg).to_owned()
        } else {
            "/".to_owned()
        });

        let task = task::get_my_current_task().ok_or(Error::CurrentTaskUnavailable)?;

        match task.get_env().lock().chdir(&path) {
            Ok(()) => Ok(()),
            Err(_) => todo!(),
        }
    }

    pub(crate) fn exec(&self, _args: Vec<&str>) -> Result<()> {
        println!("not yet implemented");
        Err(Error::Command(1))
    }

    pub(crate) fn exit(&self, _args: Vec<&str>) -> Result<()> {
        // TODO: Clean up background tasks?
        Err(Error::ExitRequested)
    }

    pub(crate) fn export(&self, _args: Vec<&str>) -> Result<()> {
        println!("not yet implemented");
        Err(Error::Command(1))
    }

    pub(crate) fn fc(&self, _args: Vec<&str>) -> Result<()> {
        println!("not yet implemented");
        Err(Error::Command(1))
    }

    pub(crate) fn fg(&self, _args: Vec<&str>) -> Result<()> {
        println!("not yet implemented");
        Err(Error::Command(1))
    }

    pub(crate) fn getopts(&self, _args: Vec<&str>) -> Result<()> {
        println!("not yet implemented");
        Err(Error::Command(1))
    }

    pub(crate) fn hash(&self, _args: Vec<&str>) -> Result<()> {
        println!("not yet implemented");
        Err(Error::Command(1))
    }

    pub(crate) fn history(&self, _args: Vec<&str>) -> Result<()> {
        println!("not yet implemented");
        Err(Error::Command(1))
    }

    pub(crate) fn jobs(&self, _args: Vec<&str>) -> Result<()> {
        println!("not yet implemented");
        Err(Error::Command(1))
    }

    pub(crate) fn set(&self, _args: Vec<&str>) -> Result<()> {
        println!("not yet implemented");
        Err(Error::Command(1))
    }

    pub(crate) fn unalias(&self, _args: Vec<&str>) -> Result<()> {
        println!("not yet implemented");
        Err(Error::Command(1))
    }

    pub(crate) fn unset(&self, _args: Vec<&str>) -> Result<()> {
        println!("not yet implemented");
        Err(Error::Command(1))
    }

    pub(crate) fn wait(&self, _args: Vec<&str>) -> Result<()> {
        println!("not yet implemented");
        Err(Error::Command(1))
    }
}
