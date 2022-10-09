#![no_std]

extern crate alloc;

mod internal;
mod wrapper;

use alloc::vec::Vec;
use core::fmt::Write;
use noline::{builder::EditorBuilder, sync::embedded::IO as Io};
use tty::Slave;

// FIXME: export main function rather than shell struct

pub struct Shell {
    slave: Slave,
}

impl Shell {
    /// Creates a new shell.
    pub fn new(slave: Slave) -> Self {
        Self { slave }
    }

    /// Runs the shell, consuming it in the process.
    pub fn run(self) -> Result<()> {
        let result = self._run();
        self.set_app_discipline();
        result
    }
}

impl Shell {
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

    fn _run(&self) -> Result<()> {
        self.set_shell_discipline();

        let wrapper = wrapper::Wrapper(&self.slave);
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

    fn execute(&self, line: &str) -> Result<()> {
        // TODO | and &

        let (cmd, args) = if let Some((cmd, args_str)) = line.split_once(" ") {
            let args = args_str.split(" ").collect::<Vec<_>>();
            (cmd, args)
        } else {
            (line, Vec::new())
        };

        let _: () = match cmd {
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
            _ => self.resolve_external(line),
        }?;

        todo!();
    }

    fn resolve_external(&self, _line: &str) -> Result<()> {
        todo!();
    }
}

pub type Result<T> = core::result::Result<T, Error>;

pub enum Error {
    Exit,
}
