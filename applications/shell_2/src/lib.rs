#![no_std]

extern crate alloc;

mod internal;
mod wrapper;

use alloc::{borrow::ToOwned, format, vec::Vec};
use core::fmt::Write;
use hashbrown::HashMap;
use noline::{builder::EditorBuilder, sync::embedded::IO as Io};
use task::{ExitValue, JoinableTaskRef, RunState, TaskRef};
use tty::Slave;

// FIXME: export main function rather than shell struct

pub struct Shell<'a> {
    slave: &'a Slave,
    // TODO: Could use a vec-based data structure like Vec<Option<JoinableTaskRef>
    // Adding a job would iterate over the vec trying to find a None and if it can't, push to the
    // end. Removing a job would replace the job with None.
    jobs: HashMap<usize, TaskRef>,
    stop_order: Vec<usize>,
}

impl<'a> Shell<'a> {
    /// Creates a new shell.
    pub fn new(slave: &'a Slave) -> Self {
        Self {
            slave,
            jobs: HashMap::new(),
            stop_order: Vec::new(),
        }
    }

    /// Runs the shell, consuming it in the process.
    pub fn run(mut self) -> Result<()> {
        let result = self._run();
        self.set_app_discipline();
        result
    }
}

impl Shell<'_> {
    /// Configures the line discipline for use by the shell.
    fn set_shell_discipline(&self) {
        let mut discipline = self.slave.discipline();
        discipline.raw();
    }

    /// Configures the line discipline for use by applications.
    fn set_app_discipline(&self) {
        let mut discipline = self.slave.discipline();
        discipline.sane();
    }

    fn _run(&mut self) -> Result<()> {
        self.set_shell_discipline();

        // TODO: Ideally don't clone
        let wrapper = wrapper::Wrapper(self.slave);
        let mut io = Io::new(wrapper);
        let mut editor = EditorBuilder::new_unbounded()
            .with_unbounded_history()
            .build_sync(&mut io)
            .unwrap();

        loop {
            editor.dedup_history();
            if let Ok(line) = editor.readline("> ", &mut io) {
                self.execute(line)?;
            } else {
                write!(io, "failed to read line").unwrap();
            }
        }
    }

    fn execute(&mut self, line: &str) -> Result<isize> {
        // TODO | and &

        let (cmd, args) = if let Some((cmd, args_str)) = line.split_once(" ") {
            let args = args_str.split(" ").collect::<Vec<_>>();
            (cmd, args)
        } else {
            (line, Vec::new())
        };

        match cmd {
            "" => Ok(0),
            "alias" => self.alias(args),
            "bg" => self.bg(args),
            "cd" => self.cd(args),
            "exec" => self.exec(args),
            "exit" => self.exit(args),
            "export" => self.export(args),
            "fc" => self.fc(args),
            "fg" => self.fg(args),
            "getopts" => self.getopts(args),
            "hash" => self.hash(args),
            "set" => self.set(args),
            "unalias" => self.set(args),
            "unset" => self.set(args),
            "wait" => self.set(args),
            _ => self.execute_external(cmd, args),
        }
    }

    // TODO: Use guards?
    fn execute_external(&mut self, cmd: &str, args: Vec<&str>) -> Result<isize> {
        self.set_app_discipline();
        let result = self._execute_external(cmd, args);
        self.set_shell_discipline();
        result
    }

    fn _execute_external(&mut self, cmd: &str, args: Vec<&str>) -> Result<isize> {
        let task = self.resolve_external(cmd, args)?;

        let mut num = 1;
        while self.jobs.contains_key(&num) {
            num += 1;
        }
        // TODO: Don't clone?
        self.jobs.insert(num, task.clone());
        self.slave.discipline().foreground(Some(task.clone()));
        task.unblock();

        loop {
            match task.runstate() {
                RunState::Suspended => {
                    self.slave.discipline().foreground(None);
                    self.stop_order.push(num);
                    return Ok(0);
                }
                RunState::Exited => {
                    self.slave.discipline().foreground(None);
                    self.jobs.remove(&num).unwrap();
                    return Ok(match task.take_exit_value().unwrap() {
                        ExitValue::Completed(status) => {
                            status.downcast_ref::<isize>().unwrap().clone()
                        }
                        ExitValue::Killed(_) => {
                            // TODO: Should we check that it was KillReason::Requested?
                            // TODO: Decide on a value. Bash uses 130.
                            1
                        }
                    });
                }
                RunState::Reaped => todo!("task reaped not by shell"),
                // TODO: Yield?
                _ => {}
            }
        }
    }

    fn resolve_external(&self, cmd: &str, args: Vec<&str>) -> Result<JoinableTaskRef> {
        let namespace_dir = task::get_my_current_task()
            .map(|t| t.get_namespace().dir().clone())
            .unwrap();
        let crate_name = format!("{}-", cmd);
        let mut matching_files = namespace_dir
            .get_files_starting_with(&crate_name)
            .into_iter();

        let app_path = matching_files
            .next()
            .map(|f| path::Path::new(f.lock().get_absolute_path()))
            .unwrap();

        if matching_files.next().is_some() {
            panic!("multiple matching files found");
        }

        // TODO: set environment
        // TODO: set io

        Ok(spawn::new_application_task_builder(app_path, None)
            .unwrap()
            .argument(
                args.into_iter()
                    .map(|arg| arg.to_owned())
                    .collect::<Vec<_>>(),
            )
            .block()
            .spawn()
            .unwrap())
    }
}

pub type Result<T> = core::result::Result<T, Error>;

pub enum Error {
    Exit,
}
