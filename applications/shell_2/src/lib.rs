#![no_std]

extern crate alloc;

mod error;
mod internal;
mod wrapper;

pub use error::{Error, Result};

use alloc::{borrow::ToOwned, format, string::String, sync::Arc, vec::Vec};
use app_io::println;
use core::fmt::Write;
use hashbrown::HashMap;
use noline::{builder::EditorBuilder, sync::embedded::IO as Io};
use path::Path;
use task::{ExitValue, JoinableTaskRef, RunState, TaskRef};
use tty::LineDiscipline;

// FIXME: export main function rather than shell struct

pub fn main(_: Vec<String>) -> isize {
    Shell::new().run().unwrap();
    0
}

pub struct Shell {
    // TODO: Make LineDiscipline interior mutable?
    discipline: LineDiscipline,
    // TODO: Could use a vec-based data structure like Vec<Option<JoinableTaskRef>
    // Adding a job would iterate over the vec trying to find a None and if it can't, push to the
    // end. Removing a job would replace the job with None.
    jobs: HashMap<usize, TaskRef>,
    stop_order: Vec<usize>,
}

impl Shell {
    /// Creates a new shell.
    pub fn new() -> Self {
        Self {
            discipline: app_io::line_discipline().unwrap(),
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

impl Shell {
    /// Configures the line discipline for use by the shell.
    fn set_shell_discipline(&self) {
        self.discipline.set_raw();
    }

    /// Configures the line discipline for use by applications.
    fn set_app_discipline(&self) {
        self.discipline.set_sane();
    }

    fn _run(&mut self) -> Result<()> {
        self.set_shell_discipline();

        // TODO: Ideally don't clone
        let wrapper = wrapper::Wrapper {
            stdin: app_io::stdin().unwrap(),
            stdout: app_io::stdout().unwrap(),
        };
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

    fn execute(&mut self, line: &str) -> Result<()> {
        // TODO | and &

        let (cmd, args) = if let Some((cmd, args_str)) = line.split_once(' ') {
            let args = args_str.split(" ").collect::<Vec<_>>();
            (cmd, args)
        } else {
            (line, Vec::new())
        };

        let result = match cmd {
            "" => Ok(()),
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
        };
        
        match result {
            Ok(()) => Ok(()),
            Err(e) if e.is_fatal() => Err(e),
            Err(e) => {
                println!("exit {}", e.exit_code());
                Ok(())
            }
        }
    }

    // TODO: Use guards?
    fn execute_external(&mut self, cmd: &str, args: Vec<&str>) -> Result<()> {
        self.set_app_discipline();
        let result = self._execute_external(cmd, args);
        self.set_shell_discipline();
        result
    }

    fn _execute_external(&mut self, cmd: &str, args: Vec<&str>) -> Result<()> {
        let task = self.resolve_external(cmd, args)?;

        let mut num = 1;
        while self.jobs.contains_key(&num) {
            num += 1;
        }
        // TODO: Don't clone?
        self.jobs.insert(num, task.clone());
        self.discipline.set_foreground(Some(task.clone()));
        task.unblock();

        loop {
            match task.runstate() {
                RunState::Suspended => {
                    self.discipline.set_foreground(None);
                    self.stop_order.push(num);
                    return Ok(());
                }
                RunState::Exited => {
                    self.discipline.set_foreground(None);
                    self.jobs.remove(&num).unwrap();
                    return match task.take_exit_value().unwrap() {
                        ExitValue::Completed(status) => {
                            match *status.downcast_ref::<isize>().unwrap() {
                                0 => Ok(()),
                                e @ _ => Err(Error::Command(e)),
                            }
                        }
                        ExitValue::Killed(_) => {
                            // TODO: Should we check that it was KillReason::Requested?
                            // TODO: Decide on a value. Bash uses 130.
                            Ok(())
                        }
                    };
                }
                RunState::Reaped => todo!("task reaped not by shell"),
                // TODO: Yield?
                _ => {}
            }
        }
    }

    fn resolve_external(&self, cmd: &str, args: Vec<&str>) -> Result<JoinableTaskRef> {
        // FIXME: Console spawns the shell in kernel namespace
        let namespace_dir = task::get_my_current_task()
            .map(|t| t.get_namespace().dir().clone())
            .expect("couldn't get namespace dir");
        // let namespace_dir = mod_mgmt::NamespaceDir::new(
        //     Path::new("/namespaces/_applications".to_owned())
        //         .get_dir(root::get_root())
        //         .unwrap(),
        // );

        let crate_name = format!("{}-", cmd);
        let mut matching_files = namespace_dir
            .get_files_starting_with(&crate_name)
            .into_iter();

        let app_path = matching_files
            .next()
            .map(|f| Path::new(f.lock().get_absolute_path()))
            .expect("couldn't find file");

        if matching_files.next().is_some() {
            panic!("multiple matching files found");
        }

        let task = spawn::new_application_task_builder(app_path, None)
            .unwrap()
            .argument(
                args.into_iter()
                    .map(|arg| arg.to_owned())
                    .collect::<Vec<_>>(),
            )
            .block()
            .spawn()
            .unwrap();

        let id = task.id;
        // TODO: Double arc :(
        app_io::insert_child_streams(id, app_io::streams().unwrap());
        // TODO: set environment

        Ok(task)
    }
}
