#![no_std]

extern crate alloc;

mod channel;
mod discipline;

use core::ops::DerefMut;

pub use discipline::LineDiscipline;

use alloc::sync::Arc;
use channel::Channel;
use core2::io::{Read, Write};
use mutex_sleep::MutexSleep as Mutex;

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
    discipline: Arc<Mutex<LineDiscipline>>,
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
pub struct Master {
    master: Channel,
    slave: Channel,
    discipline: Arc<Mutex<LineDiscipline>>,
}

impl Master {
    pub fn discipline(&self) -> impl DerefMut<Target = LineDiscipline> + '_ {
        self.discipline.lock().unwrap()
    }

    pub fn read(&self, buf: &mut [u8]) -> usize {
        self.master.receive_buf(buf)
    }

    pub fn read_byte(&self) -> u8 {
        self.master.receive()
    }

    pub fn write(&self, buf: &[u8]) -> usize {
        if buf.is_empty() {
            return 0;
        }

        let mut discipline = self.discipline.lock().unwrap();
        discipline.process_slave_in(buf, &self.master, &self.slave);
        buf.len()
    }

    pub fn write_byte(&self, byte: u8) {
        self.write(&[byte]);
    }
}

impl Read for Master {
    fn read(&mut self, buf: &mut [u8]) -> core2::io::Result<usize> {
        Ok(Self::read(self, buf))
    }
}

impl Write for Master {
    fn write(&mut self, buf: &[u8]) -> core2::io::Result<usize> {
        Ok(Self::write(self, buf))
    }

    fn flush(&mut self) -> core2::io::Result<()> {
        todo!("do we flush canonical buffer?");
    }
}

/// The slave (i.e. application) end of a [`Tty`].
pub struct Slave {
    master: Channel,
    slave: Channel,
    discipline: Arc<Mutex<LineDiscipline>>,
}

impl Slave {
    pub fn discipline(&self) -> impl DerefMut<Target = LineDiscipline> + '_ {
        self.discipline.lock().unwrap()
    }

    pub fn read(&self, buf: &mut [u8]) -> usize {
        self.slave.receive_buf(buf)
    }

    pub fn read_byte(&self) -> u8 {
        self.slave.receive()
    }

    pub fn write(&self, buf: &[u8]) -> usize {
        self.slave.send_buf(buf);
        buf.len()
    }

    pub fn write_byte(&self, byte: u8) {
        self.master.send(byte);
    }
}

impl Read for Slave {
    fn read(&mut self, buf: &mut [u8]) -> core2::io::Result<usize> {
        Ok(Self::read(self, buf))
    }
}

impl Write for Slave {
    fn write(&mut self, buf: &[u8]) -> core2::io::Result<usize> {
        Ok(Self::write(self, buf))
    }

    fn flush(&mut self) -> core2::io::Result<()> {
        Ok(())
    }
}
