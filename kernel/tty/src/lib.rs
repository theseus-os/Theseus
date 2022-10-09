//! ```text
//!       +--- terminal <---+
//!       |                 |
//!       |             master.read()
//!       V                 |
//! master.write() --> master.reader
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

mod channel;
mod discipline;

use core::ops::DerefMut;

pub use discipline::LineDiscipline;

use alloc::sync::Arc;
use channel::Channel;
use core2::io::{Read, Write};
use mutex_sleep::MutexSleep as Mutex;

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
        if buf.len() == 0 {
            return 0;
        }
        
        log::debug!("writing");

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
