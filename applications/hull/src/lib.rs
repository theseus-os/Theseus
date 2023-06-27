//! `hull` is Theseus's shell for basic interactive systems operations.
//!
//! Just as the hull is the outermost layer or "shell" of a boat or ship,
//! this crate `hull` is the shell of the "Ship of Theseus" (this OS).
//!
//! Functionally, this is similar to bash, zsh, fish, etc.
//!
//! This shell will eventually supercede the shell located at
//! `applications/shell`.

#![no_std]

extern crate alloc;

mod builtin;
mod error;
mod job;
mod wrapper;

pub use error::{Error, Result};

use crate::job::{JobPart, State};
use alloc::{borrow::ToOwned, format, string::String, sync::Arc, vec::Vec};
use app_io::println;
use atomic_linked_list::atomic_map::AtomicMap;
use core::{fmt::Write, mem};
use job::Job;
use noline::{builder::EditorBuilder, sync::embedded::IO as Io};
use path::Path;
use tty::{Event, LineDiscipline};

pub fn main(_: Vec<String>) -> isize {
    let mut shell = Shell {
        discipline: app_io::line_discipline().expect("no line discipline"),
        jobs: Arc::new(AtomicMap::new()),
        stop_order: Vec::new(),
        history: Vec::new(),
    };
    let result = shell.run();
    shell.set_app_discipline();
    if let Err(e) = result {
        println!("{e:?}");
        -1
    } else {
        0
    }
}

pub struct Shell {
    discipline: Arc<LineDiscipline>,
    // TODO: Could use a vec-based data structure like Vec<Option<JoinableTaskRef>
    // Adding a job would iterate over the vec trying to find a None and if it can't, push to the
    // end. Removing a job would replace the job with None.
    jobs: Arc<AtomicMap<usize, Job>>,
    stop_order: Vec<usize>,
    history: Vec<String>,
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

    fn run(&mut self) -> Result<()> {
        self.set_shell_discipline();

        let wrapper = wrapper::Wrapper {
            stdin: app_io::stdin().expect("no stdin"),
            stdout: app_io::stdout().expect("no stdout"),
        };
        let mut io = Io::new(wrapper);
        let mut editor = EditorBuilder::new_unbounded()
            .with_unbounded_history()
            .build_sync(&mut io)
            .expect("couldn't instantiate line editor");

        loop {
            editor.dedup_history();
            if let Ok(line) = editor.readline("> ", &mut io) {
                match self.execute_line(line) {
                    Ok(()) => {}
                    Err(Error::ExitRequested) => return Ok(()),
                    Err(e) => return Err(e),
                };
            } else {
                write!(io, "failed to read line").expect("failed to write output");
            }
        }
    }

    fn execute_line(&mut self, line: &str) -> Result<()> {
        // TODO: | and &

        self.history.push(line.to_owned());

        let mut temp_job = Job {
            line: line.to_owned(),
            parts: Vec::new(),
        };
        for cmd in parse_line(line) {
            match self.execute_cmd(cmd, &mut temp_job) {
                Ok(()) => continue,
                Err(Error::ExitRequested) => return Err(Error::ExitRequested),
                Err(Error::CurrentTaskUnavailable) => return Err(Error::CurrentTaskUnavailable),
                Err(Error::Command(exit_code)) => println!("exit {}", exit_code),
                Err(Error::CommandNotFound(command)) => println!("{}: command not found", command),
                Err(Error::SpawnFailed(s)) => println!("failed to spawn task: {s}"),
                Err(Error::KillFailed) => println!("failed to kill task"),
                Err(Error::UnblockFailed(state)) => {
                    println!("failed to unblock task with state {:?}", state)
                }
            }
        }

        Ok(())
    }

    fn execute_cmd(&mut self, cmd: Command, temp_job: &mut Job) -> Result<()> {
        match cmd {
            // TODO: Handle internal backgrounded commands.
            Command::Backgrounded(cmd, args) => {
                let job_part = self.resolve_external(cmd, args)?;
                temp_job.parts.push(job_part);
                let mut job = mem::take(temp_job);
                job.unblock()?;
                self.insert_job(job);
                Ok(())
            }
            // TODO: Handle internal piped commands.
            Command::Piped(cmd, args) => {
                let job_part = self.resolve_external(cmd, args)?;
                todo!("handle io");
                temp_job.parts.push(job_part);
            }
            Command::None(cmd, args) => {
                if let Some(result) = self.execute_builtin(cmd, &args) {
                    result
                } else {
                    let job_part = self.resolve_external(cmd, args)?;
                    temp_job.parts.push(job_part);
                    let mut job = mem::take(temp_job);
                    job.unblock()?;
                    let num = self.insert_job(job);
                    self.wait_on_job(num)?;
                    Ok(())
                }
            }
        }
    }

    // We can't do anything use
    fn insert_job(&mut self, mut job: Job) -> usize {
        let mut num = 1;
        loop {
            match self.jobs.try_insert(num, job) {
                Ok(_) => return num,
                Err(e) => {
                    job = e.value;
                }
            }
            num += 1;
        }
    }

    fn wait_on_job(&mut self, num: usize) -> Result<()> {
        let Some(job) = self.jobs.get_mut(num) else {
            return Ok(())
        };

        self.discipline.clear_events();
        loop {
            // TODO: Use async futures::select! loop?
            if let Ok(event) = self.discipline.event_receiver().try_receive() {
                return match event {
                    Event::CtrlC => {
                        job.kill()?;
                        // self.jobs.remove(&num);
                        Err(Error::Command(130))
                    }
                    Event::CtrlD => todo!(),
                    Event::CtrlZ => {
                        job.suspend();
                        todo!();
                    }
                };
            } else if let Some(exit_value) = job.update() {
                // self.jobs.remove(&num);
                return match exit_value {
                    0 => Ok(()),
                    _ => Err(Error::Command(exit_value)),
                };
            }
        }
    }

    fn execute_builtin(&mut self, cmd: &str, args: &Vec<&str>) -> Option<Result<()>> {
        Some(match cmd {
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
            "history" => {
                self.history(args);
                Ok(())
            }
            "jobs" => self.jobs(args),
            "set" => self.set(args),
            "unalias" => self.unalias(args),
            "unset" => self.unset(args),
            "wait" => self.wait(args),
            _ => return None,
        })
    }

    fn resolve_external(&self, cmd: &str, args: Vec<&str>) -> Result<JobPart> {
        let namespace_dir = task::get_my_current_task()
            .map(|t| t.get_namespace().dir().clone())
            .expect("couldn't get namespace dir");

        let crate_name = format!("{cmd}-");
        let mut matching_files = namespace_dir
            .get_files_starting_with(&crate_name)
            .into_iter();

        let app_path = match matching_files.next() {
            Some(f) => Path::new(f.lock().get_absolute_path()),
            None => return Err(Error::CommandNotFound(cmd.to_owned())),
        };

        if matching_files.next().is_some() {
            println!("multiple matching files found, running: {app_path}");
        }

        let task = spawn::new_application_task_builder(app_path, None, || {
            if self.jobs.remove(todo!()).is_err() {
                error!("job {id} not present in jobs list");
            }
        })
        .map_err(Error::SpawnFailed)?
        .argument(args.into_iter().map(ToOwned::to_owned).collect::<Vec<_>>())
        .block()
        .spawn()
        .unwrap();

        let id = task.id;
        // TODO: Double arc :(
        app_io::insert_child_streams(id, app_io::streams().unwrap());

        // task.unblock().map_err(Error::UnblockFailed)?;

        Ok(JobPart {
            state: State::Running,
            task,
        })
    }
}

enum Command<'a> {
    Backgrounded(&'a str, Vec<&'a str>),
    Piped(&'a str, Vec<&'a str>),
    None(&'a str, Vec<&'a str>),
}

fn parse_line(mut line: &str) -> Vec<Command<'_>> {
    let mut result = Vec::new();

    // TODO: Error when last command is piped
    loop {
        match (line.split_once('|'), line.split_once('&')) {
            (Some((a, rem_1)), Some((b, rem_2))) => {
                if a.len() < b.len() {
                    let (cmd, args) = split_args(a.trim());
                    result.push(Command::Piped(cmd, args));
                    line = rem_1.trim();
                } else {
                    let (cmd, args) = split_args(b.trim());
                    result.push(Command::Backgrounded(cmd, args));
                    line = rem_2.trim();
                }
            }
            (Some((a, rem)), None) => {
                let (cmd, args) = split_args(a.trim());
                result.push(Command::Piped(cmd, args));
                line = rem.trim();
            }
            (None, Some((b, rem))) => {
                let (cmd, args) = split_args(b.trim());
                result.push(Command::Backgrounded(cmd, args));
                line = rem.trim();
            }
            (None, None) => break,
        }
    }

    let trimmed = line.trim();
    if !trimmed.is_empty() {
        let (cmd, args) = split_args(trimmed);
        result.push(Command::None(cmd, args));
    }

    result
}

fn split_args(line: &str) -> (&str, Vec<&str>) {
    // TODO: Handle backslashes and quotes.
    if let Some((cmd, args_str)) = line.split_once(' ') {
        let args = args_str.split(' ').collect::<Vec<_>>();
        (cmd, args)
    } else {
        (line, Vec::new())
    }
}

struct AppDisciplineGuard<'a> {
    discipline: &'a LineDiscipline,
}

impl<'a> Drop for AppDisciplineGuard<'a> {
    fn drop(&mut self) {
        self.discipline.set_raw();
    }
}
