#![feature(never_type)]
#![no_std]

mod wrapper;

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

    /// Runs the shell, consuming it in the process.
    pub fn run(self) {
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
                write!(io, "read: '{}'\n\r", line).unwrap();
            } else {
                write!(io, "failed to read line").unwrap();
            }
        }
    }
}
