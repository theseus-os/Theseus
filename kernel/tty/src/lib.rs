//! ```text
//!       +--- terminal <---+
//!       |                 |
//!       V                 |
//! master.writer --> master.reader
//!       |                 ^
//!       |                 |
//!       V                 |
//! slave.reader       slave.writer
//!       |                 ^
//!       |                 |
//!       +-----> app ------+
//! ```
//! The line discipline functionality is split between `master.writer` and
//! `slave.reader`.

#![no_std]

extern crate alloc;

mod discipline;

use core2::io::{Read, Write};
use discipline::{MasterDiscipline, SlaveDiscipline};
use stdio::{Stdio, StdioReader as Reader, StdioWriter as Writer};

pub struct Master {
    /// Writes to master reader and slave reader.
    writer: MasterDiscipline,
    /// Reads from master writer and slaver writer.
    reader: Reader,
}

impl Write for Master {
    fn write(&mut self, buf: &[u8]) -> core2::io::Result<usize> {
        self.writer.write(buf)
    }

    fn flush(&mut self) -> core2::io::Result<()> {
        self.writer.flush()
    }
}

impl Read for Master {
    fn read(&mut self, buf: &mut [u8]) -> core2::io::Result<usize> {
        self.reader.lock().read(buf)
    }
}

pub struct Slave {
    /// Writes to master reader.
    writer: Writer,
    /// Reads from master writer.
    reader: SlaveDiscipline,
}

impl Write for Slave {
    fn write(&mut self, buf: &[u8]) -> core2::io::Result<usize> {
        self.writer.lock().write(buf)
    }

    fn flush(&mut self) -> core2::io::Result<()> {
        self.writer.lock().flush()
    }
}

impl Read for Slave {
    fn read(&mut self, buf: &mut [u8]) -> core2::io::Result<usize> {
        self.reader.read(buf)
    }
}

pub fn tty() -> (Master, Slave) {
    let master_stdio = Stdio::new();
    let (master_reader, master_writer) = (master_stdio.get_reader(), master_stdio.get_writer());

    let slave_stdio = Stdio::new();
    let (slave_reader, slave_writer) = (slave_stdio.get_reader(), slave_stdio.get_writer());

    (
        Master {
            writer: MasterDiscipline { master: master_writer.clone(), slave: slave_writer },
            reader: master_reader,
        },
        Slave { writer: master_writer, reader: SlaveDiscipline::new(slave_reader) },
    )
}
