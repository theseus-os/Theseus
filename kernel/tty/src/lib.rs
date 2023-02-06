//! This crate provides TTY abstractions.
//!
//! TTYs define the interface between a terminal and the application running in
//! the terminal.

#![no_std]

extern crate alloc;

mod channel;
mod discipline;

pub use discipline::{Event, LineDiscipline};

use alloc::sync::Arc;
use channel::Channel;
use core2::io::{Read, Result, Write};

/// A terminal device driver.
///
/// The design is based on the Unix TTY/PTY subsystem. Unlike Unix, Theseus does
/// not distinguish between teletypes and pseudo-teletypes. Each `Tty` consists
/// of two ends: a [`Master`] and a [`Slave`]. The terminal holds the master and
/// the application holds the slave. The TTY's [`LineDiscipline`] dictates how
/// the two interact.
///
/// In the context of Theseus, there are two terminals:
/// - `terminal_emulator`, which is the graphical terminal emulator implemented
///   in Theseus.
/// - `console`, which connects to an external terminal emulator through a
///   serial port.
///
/// When started, both terminals launch the `shell`, which contains the logic to
/// launch applications, store command history, autocomplete input, etc.
#[derive(Clone)]
pub struct Tty {
    master: Channel,
    slave: Channel,
    discipline: Arc<LineDiscipline>,
}

impl Default for Tty {
    fn default() -> Self {
        Self::new()
    }
}

impl Tty {
    pub fn new() -> Self {
        Self {
            master: Channel::new(),
            slave: Channel::new(),
            discipline: Default::default(),
        }
    }

    pub fn master(&self) -> Master {
        Master {
            master: self.master.clone(),
            slave: self.slave.clone(),
            discipline: self.discipline.clone(),
        }
    }

    pub fn slave(&self) -> Slave {
        Slave {
            master: self.master.clone(),
            slave: self.slave.clone(),
            discipline: self.discipline.clone(),
        }
    }
}

/// The master (i.e. terminal) end of a [`Tty`].
#[derive(Clone)]
pub struct Master {
    master: Channel,
    slave: Channel,
    discipline: Arc<LineDiscipline>,
}

impl Master {
    pub fn discipline(&self) -> Arc<LineDiscipline> {
        self.discipline.clone()
    }

    pub fn read_byte(&self) -> Result<u8> {
        self.master.receive()
    }

    pub fn read(&self, buf: &mut [u8]) -> Result<usize> {
        self.master.receive_buf(buf)
    }

    pub fn try_read(&self, buf: &mut [u8]) -> Result<usize> {
        self.master.try_receive_buf(buf)
    }

    pub fn write_byte(&self, byte: u8) -> Result<()> {
        self.discipline
            .process_input_byte(byte, &self.master, &self.slave)?;
        Ok(())
    }

    pub fn write(&self, buf: &[u8]) -> Result<usize> {
        // TODO: Don't fail if we can't send entire buf.
        self.discipline
            .process_input_buf(buf, &self.master, &self.slave)?;
        Ok(buf.len())
    }
}

impl Read for Master {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        let immutable: &Self = self;
        immutable.read(buf)
    }
}

impl Write for Master {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        let immutable: &Self = self;
        immutable.write(buf)
    }

    fn flush(&mut self) -> core2::io::Result<()> {
        todo!("do we flush canonical buffer?");
    }
}

/// The slave (i.e. application) end of a [`Tty`].
#[derive(Clone)]
pub struct Slave {
    master: Channel,
    slave: Channel,
    discipline: Arc<LineDiscipline>,
}

impl Slave {
    pub fn discipline(&self) -> Arc<LineDiscipline> {
        self.discipline.clone()
    }

    pub fn read_byte(&self) -> Result<u8> {
        self.slave.receive()
    }

    pub fn read(&self, buf: &mut [u8]) -> Result<usize> {
        self.slave.receive_buf(buf)
    }

    pub fn try_read(&self, buf: &mut [u8]) -> Result<usize> {
        self.slave.try_receive_buf(buf)
    }

    pub fn write_byte(&self, byte: u8) -> Result<()> {
        self.discipline.process_output_byte(byte, &self.master)?;
        Ok(())
    }

    pub fn write(&self, buf: &[u8]) -> Result<usize> {
        // TODO: Don't fail if we can't send entire buf.
        self.discipline.process_output_buf(buf, &self.master)?;
        Ok(buf.len())
    }
}

impl Read for Slave {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        let immutable: &Self = self;
        immutable.read(buf)
    }
}

impl Write for Slave {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        let immutable: &Self = self;
        immutable.write(buf)
    }

    fn flush(&mut self) -> Result<()> {
        Ok(())
    }
}
