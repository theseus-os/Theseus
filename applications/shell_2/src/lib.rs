#![no_std]

extern crate alloc;

mod error;
mod internal;
mod job;
mod wrapper;

pub use error::{Error, Result};

use crate::job::{JobPart, State};
use alloc::{borrow::ToOwned, format, string::String, sync::Arc, vec, vec::Vec};
use app_io::println;
use core::fmt::Write;
use hashbrown::HashMap;
use job::Job;
use noline::{builder::EditorBuilder, sync::embedded::IO as Io};
use path::Path;
use tty::{Event, LineDiscipline};

pub fn main(_: Vec<String>) -> isize {
    Shell::new().run().unwrap();
    0
}

pub struct Shell {
    discipline: Arc<LineDiscipline>,
    // TODO: Could use a vec-based data structure like Vec<Option<JoinableTaskRef>
    // Adding a job would iterate over the vec trying to find a None and if it can't, push to the
    // end. Removing a job would replace the job with None.
    jobs: HashMap<usize, Job>,
    stop_order: Vec<usize>,
}

impl Default for Shell {
    fn default() -> Self {
        Self::new()
    }
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
                match self.execute(line) {
                    Ok(()) => {}
                    Err(Error::ExitRequested) => return Ok(()),
                    Err(e) => return Err(e),
                };
            } else {
                write!(io, "failed to read line").unwrap();
            }
        }
    }

    fn execute(&mut self, line: &str) -> Result<()> {
        // TODO | and &

        let (cmd, args) = if let Some((cmd, args_str)) = line.split_once(' ') {
            let args = args_str.split(' ').collect::<Vec<_>>();
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
            "history" => self.history(args),
            "jobs" => self.jobs(args),
            "set" => self.set(args),
            "unalias" => self.unalias(args),
            "unset" => self.unset(args),
            "wait" => self.wait(args),
            _ => self.execute_external(cmd, args),
        };

        match result {
            Ok(()) => Ok(()),
            Err(Error::ExitRequested) | Err(Error::CurrentTaskUnavailable) => result,
            Err(Error::Command(exit_code)) => {
                println!("exit {}", exit_code);
                Ok(())
            }
            Err(Error::CommandNotFound(command)) => {
                println!("{}: command not found", command);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    // TODO: Use guards to reset line disciplines rather than an extra function.
    fn execute_external(&mut self, cmd: &str, args: Vec<&str>) -> Result<()> {
        self.set_app_discipline();
        let result = self._execute_external(cmd, args);
        self.set_shell_discipline();
        result
    }

    fn _execute_external(&mut self, cmd: &str, args: Vec<&str>) -> Result<()> {
        let mut job = self.resolve_external(cmd, args)?;

        let mut num = 1;
        while self.jobs.contains_key(&num) {
            num += 1;
        }

        job.unsuspend();
        let job = self.jobs.try_insert(num, job).unwrap();
        self.discipline.clear_events();

        loop {
            if let Ok(event) = self.discipline.event_receiver().try_receive() {
                return match event {
                    Event::CtrlC => {
                        job.kill();
                        self.jobs.remove(&num).unwrap();
                        Err(Error::Command(130))
                    }
                    Event::CtrlD => todo!(),
                    Event::CtrlZ => {
                        job.suspend();
                        todo!();
                    }
                };
            } else if let Some(exit_value) = job.update() {
                self.jobs.remove(&num).unwrap();
                return match exit_value {
                    0 => Ok(()),
                    _ => Err(Error::Command(exit_value)),
                };
            }
        }
    }

    fn resolve_external(&self, cmd: &str, args: Vec<&str>) -> Result<Job> {
        let namespace_dir = task::get_my_current_task()
            .map(|t| t.get_namespace().dir().clone())
            .expect("couldn't get namespace dir");

        let crate_name = format!("{}-", cmd);
        let mut matching_files = namespace_dir
            .get_files_starting_with(&crate_name)
            .into_iter();

        let app_path = match matching_files.next() {
            Some(f) => Path::new(f.lock().get_absolute_path()),
            None => return Err(Error::CommandNotFound(cmd.to_owned())),
        };

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

        Ok(Job {
            parts: vec![JobPart {
                state: State::Running,
                task,
            }],
        })
    }
}
