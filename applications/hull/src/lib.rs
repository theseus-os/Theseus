//! `hull` is Theseus's shell for basic interactive systems operations.
//!
//! Just as the hull is the outermost layer or "shell" of a boat or ship,
//! this crate `hull` is the shell of the "Ship of Theseus" (this OS).
//!
//! Functionally, this is similar to bash, zsh, fish, etc.
//!
//! This shell will eventually supercede the shell located at
//! `applications/shell`.
//!
//! Terminology used in this file using `sleep 1 | sleep 2 & sleep 3` as an
//! example:
//! - A line is an entire line of user input i.e. `sleep 1 | sleep 2 & sleep 3`.
//! - A task is a subset of a line used to spawn an individual task i.e. `sleep
//!   1`, `sleep 2`, and `sleep 3`.
//! - A job is a list of piped tasks i.e. `sleep 1 | sleep 2`, and `sleep 3`.
//! - A command is the first word in a task i.e. `sleep`.
//! - The arguments are any subsequent words in a task i.e. `1`, `2`, and `3`.

#![cfg_attr(not(test), no_std)]
#![feature(extend_one, let_chains)]

extern crate alloc;

mod builtin;
mod error;
mod job;
mod parse;
mod wrapper;

use crate::{
    job::{JobPart, State},
    parse::{ParsedJob, ParsedLine, ParsedTask},
};
use alloc::{borrow::ToOwned, format, string::String, sync::Arc, vec::Vec};
use app_io::{println, IoStreams};
use core::fmt::Write;
use hashbrown::HashMap;
use job::Job;
use log::{error, warn};
use noline::{builder::EditorBuilder, sync::embedded::IO as Io};
use path::Path;
use stdio::Stdio;
use sync_block::Mutex;
use task::{ExitValue, KillReason};
use tty::{Event, LineDiscipline};

pub use crate::error::{Error, Result};

pub fn main(_: Vec<String>) -> isize {
    let mut shell = Shell {
        discipline: app_io::line_discipline().expect("no line discipline"),
        jobs: Arc::new(Mutex::new(HashMap::new())),
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
    jobs: Arc<Mutex<HashMap<usize, Job>>>,
    stop_order: Vec<usize>,
    history: Vec<String>,
}

impl Shell {
    /// Configures the line discipline for use by the shell.
    fn set_shell_discipline(&self) {
        self.discipline.set_raw();
    }

    /// Configures the line discipline for use by applications.
    fn set_app_discipline(&self) -> AppDisciplineGuard {
        self.discipline.set_sane();
        AppDisciplineGuard {
            discipline: self.discipline.clone(),
        }
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
        let parsed_line = ParsedLine::from(line);

        if parsed_line.is_empty() {
            return Ok(());
        }

        // TODO: Use line editor history.
        self.history.push(line.to_owned());

        for (job_str, job) in parsed_line.background {
            if let Err(error) = self.execute_cmd(job, job_str, false) {
                error.print()?;
            }
        }

        if let Some((job_str, job)) = parsed_line.foreground {
            let app_discipline_guard = self.set_app_discipline();
            match self.execute_cmd(job, job_str, true) {
                Ok(Some(foreground_id)) => {
                    if let Err(error) = self.wait_on_job(foreground_id) {
                        error.print()?;
                    }
                }
                Ok(None) => {}
                Err(error) => error.print()?,
            }
            drop(app_discipline_guard);
        }

        Ok(())
    }

    /// Executes a command.
    fn execute_cmd(
        &mut self,
        parsed_job: ParsedJob,
        job_str: &str,
        current: bool,
    ) -> Result<Option<usize>> {
        let shell_streams = app_io::streams().unwrap();

        let stderr = shell_streams.stderr;
        let mut previous_output = shell_streams.stdin;

        let mut iter = parsed_job.into_iter().peekable();
        let mut task = iter.next();

        let mut jobs = self.jobs.lock();
        let mut job_id = 1;
        let mut temp_job = Job {
            string: job_str.to_owned(),
            parts: Vec::new(),
            current,
        };
        loop {
            match jobs.try_insert(job_id, temp_job) {
                Ok(_) => break,
                Err(e) => {
                    temp_job = e.value;
                }
            }
            job_id += 1;
        }
        drop(jobs);

        while let Some(ParsedTask { command, args }) = task {
            if iter.peek().is_none() {
                if let Some(result) = self.execute_builtin(command, &args) {
                    self.jobs.lock().remove(&job_id);
                    return result.map(|_| None);
                } else {
                    let streams = IoStreams {
                        // TODO: Technically clone not needed.
                        stdin: previous_output.clone(),
                        stdout: shell_streams.stdout.clone(),
                        stderr: stderr.clone(),
                        discipline: shell_streams.discipline,
                    };
                    let part = self.resolve_external(command, args, streams, job_id)?;
                    self.jobs.lock().get_mut(&job_id).unwrap().parts.push(part);
                    return Ok(Some(job_id));
                }
            }

            // TODO: Piped builtin commands.

            let pipe = Stdio::new();
            let streams = IoStreams {
                stdin: previous_output.clone(),
                stdout: Arc::new(pipe.get_writer()),
                stderr: stderr.clone(),
                discipline: None,
            };
            let part = self.resolve_external(command, args, streams, job_id)?;
            self.jobs.lock().get_mut(&job_id).unwrap().parts.push(part);

            previous_output = Arc::new(pipe.get_reader());
            task = iter.next();
        }

        unreachable!("called execute_cmd with empty command");
    }

    fn wait_on_job(&mut self, num: usize) -> Result<()> {
        let jobs = self.jobs.lock();
        let Some(job) = jobs.get(&num) else {
            return Ok(())
        };
        if !job.current {
            warn!("asked to wait on non-current job");
            return Ok(());
        }
        drop(jobs);

        self.discipline.clear_events();
        let event_receiver = self.discipline.event_receiver();
        loop {
            // TODO: Use async futures::select! loop?
            if let Ok(event) = event_receiver.try_receive() {
                match event {
                    Event::CtrlC => {
                        if let Some(mut job) = self.jobs.lock().remove(&num) {
                            job.kill()?;
                        } else {
                            error!("tried to kill a job that doesn't exist");
                        }
                        return Err(Error::Command(130));
                    }
                    Event::CtrlD => error!("received ctrl+d event"),
                    Event::CtrlZ => error!("received ctrl+z event"),
                }
            } else {
                let mut jobs = self.jobs.lock();
                if let Some(job) = jobs.get_mut(&num)
                    && let Some(exit_value) = job.exit_value()
                {
                        jobs.remove(&num);
                        return match exit_value {
                            0 => Ok(()),
                            _ => Err(Error::Command(exit_value)),
                        };
                }
            }
            scheduler::schedule();
        }
    }

    fn execute_builtin(&mut self, cmd: &str, args: &[&str]) -> Option<Result<()>> {
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

    fn resolve_external(
        &self,
        cmd: &str,
        args: Vec<&str>,
        streams: IoStreams,
        job_id: usize,
    ) -> Result<JobPart> {
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

        let task = spawn::new_application_task_builder(app_path, None)
            .map_err(Error::SpawnFailed)?
            .argument(args.into_iter().map(ToOwned::to_owned).collect::<Vec<_>>())
            .block()
            .spawn()
            .unwrap();
        let task_ref = task.clone();

        let id = task.id;
        // TODO: Double arc :(
        app_io::insert_child_streams(id, streams);
        task.unblock().map_err(Error::UnblockFailed)?;

        // Spawn watchdog task.
        spawn::new_task_builder(
            move |_| {
                let task_ref = task.clone();

                let exit_value = match task.join().unwrap() {
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

                let mut jobs = self.jobs.lock();
                if let Some(mut job) = jobs.remove(&job_id) {
                    for part in job.parts.iter_mut() {
                        if part.task == task_ref {
                            part.state = State::Done(exit_value);
                            break;
                        }
                    }

                    if job.current {
                        jobs.insert(job_id, job);
                    }
                }
            },
            (),
        )
        .spawn()
        .map_err(Error::SpawnFailed)?;

        Ok(JobPart {
            state: State::Running,
            task: task_ref,
        })
    }
}

struct AppDisciplineGuard {
    discipline: Arc<LineDiscipline>,
}

impl Drop for AppDisciplineGuard {
    fn drop(&mut self) {
        self.discipline.set_raw();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn test_split_pipes() {
        assert_eq!(
            split_pipes("a b c |d e f|g | h | i j"),
            vec![
                ("a", vec!["b", "c"]),
                ("d", vec!["e", "f"]),
                ("g", vec![]),
                ("h", vec![]),
                ("i", vec!["j"])
            ]
        );
    }

    #[test]
    fn test_parse_line() {
        assert_eq!(
            parse_line("a b|c  &d e f|g | h & i j | k"),
            ParsedLine {
                background: vec![
                    vec![("a", vec!["b"]), ("c", vec![])],
                    vec![("d", vec!["e", "f"]), ("g", vec![]), ("h", vec![])],
                ],
                foreground: Some(vec![("i", vec!["j"]), ("k", vec![])]),
            }
        );
        assert_eq!(
            parse_line("a b|c  &d e f|g | h & i j | k&  "),
            ParsedLine {
                background: vec![
                    vec![("a", vec!["b"]), ("c", vec![])],
                    vec![("d", vec!["e", "f"]), ("g", vec![]), ("h", vec![])],
                    vec![("i", vec!["j"]), ("k", vec![])]
                ],
                foreground: None,
            }
        );
    }
}
