//! Builtin shell commands.

use crate::{Error, Result, Shell};
use alloc::{borrow::ToOwned, string::ToString};
use app_io::println;
use path::Path;

// TODO: Decide which builtins we don't need.

impl Shell {
    pub(crate) fn alias(&self, _args: &[&str]) -> Result<()> {
        println!("not yet implemented");
        Err(Error::Command(1))
    }

    pub(crate) fn bg(&mut self, args: &[&str]) -> Result<()> {
        let mut jobs = self.jobs.lock();
        if args.is_empty() {
            loop {
                let num = match self.stop_order.pop() {
                    Some(n) => n,
                    None => {
                        println!("no current job");
                        return Err(Error::Command(1));
                    }
                };
                if let Some(task) = jobs.get_mut(&num) {
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
                    let task = match jobs.get_mut(&num) {
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

    pub(crate) fn cd(&self, args: &[&str]) -> Result<()> {
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
            Err(_) => {
                println!("no such file or directory: {path}");
                Err(Error::Command(1))
            }
        }
    }

    pub(crate) fn exec(&self, _args: &[&str]) -> Result<()> {
        println!("not yet implemented");
        Err(Error::Command(1))
    }

    pub(crate) fn exit(&self, _args: &[&str]) -> Result<()> {
        // TODO: Clean up background tasks?
        Err(Error::ExitRequested)
    }

    pub(crate) fn export(&self, _args: &[&str]) -> Result<()> {
        println!("not yet implemented");
        Err(Error::Command(1))
    }

    pub(crate) fn fc(&self, _args: &[&str]) -> Result<()> {
        println!("not yet implemented");
        Err(Error::Command(1))
    }

    pub(crate) fn fg(&self, _args: &[&str]) -> Result<()> {
        println!("not yet implemented");
        Err(Error::Command(1))
    }

    pub(crate) fn getopts(&self, _args: &[&str]) -> Result<()> {
        println!("not yet implemented");
        Err(Error::Command(1))
    }

    pub(crate) fn hash(&self, _args: &[&str]) -> Result<()> {
        println!("not yet implemented");
        Err(Error::Command(1))
    }

    pub(crate) fn history(&self, _args: &[&str]) {
        let num_column_max_length = self.history.len().to_string().len() + 1;

        for (i, line) in self
            .history
            .iter()
            .enumerate()
            .map(|(i, line)| (i + 1, line))
        {
            let num_string = i.to_string();
            let padding = " ".repeat(num_column_max_length - num_string.len());
            println!("{padding}{num_string} {line}");
        }
    }

    pub(crate) fn jobs(&self, _: &[&str]) -> Result<()> {
        // TODO: Sort IDs.
        for (id, job) in self.jobs.lock().iter() {
            // TODO: Separate job parts if they are in different states.
            let Some(state) = &job.parts.get(0).map(|part| &part.state) else {
                continue;
            };
            let line = &job.string;

            println!("[{id}]    {state}    {line}");
        }
        Ok(())
    }

    pub(crate) fn set(&self, _args: &[&str]) -> Result<()> {
        println!("not yet implemented");
        Err(Error::Command(1))
    }

    pub(crate) fn unalias(&self, _args: &[&str]) -> Result<()> {
        println!("not yet implemented");
        Err(Error::Command(1))
    }

    pub(crate) fn unset(&self, _args: &[&str]) -> Result<()> {
        println!("not yet implemented");
        Err(Error::Command(1))
    }

    pub(crate) fn wait(&self, _args: &[&str]) -> Result<()> {
        println!("not yet implemented");
        Err(Error::Command(1))
    }
}
