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

mod discipline;

use core::ops::DerefMut;

pub use discipline::LineDiscipline;

use alloc::{collections::VecDeque, sync::Arc};
use core2::io::{Read, Write};
use mutex_sleep::MutexSleep as Mutex;

#[derive(Clone, Default)]
pub struct Tty {
    master_in: Arc<Mutex<VecDeque<u8>>>,
    slave_in: Arc<Mutex<VecDeque<u8>>>,
    discipline: Arc<Mutex<LineDiscipline>>,
}

impl Tty {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn master(&self) -> Master {
        Master {
            master_in: self.master_in.clone(),
            slave_in: self.slave_in.clone(),
            discipline: self.discipline.clone(),
        }
    }

    pub fn slave(&self) -> Slave {
        Slave {
            master_in: self.master_in.clone(),
            slave_in: self.slave_in.clone(),
            discipline: self.discipline.clone(),
        }
    }
}

pub struct Master {
    master_in: Arc<Mutex<VecDeque<u8>>>,
    slave_in: Arc<Mutex<VecDeque<u8>>>,
    discipline: Arc<Mutex<LineDiscipline>>,
}

impl Master {
    pub fn discipline(&self) -> impl DerefMut<Target = LineDiscipline> + '_ {
        self.discipline.lock().unwrap()
    }
}

impl Read for Master {
    fn read(&mut self, buf: &mut [u8]) -> core2::io::Result<usize> {
        let mut master_in = self.master_in.lock().unwrap();
        let (s_1, s_2) = master_in.as_slices();

        let mut n = core::cmp::min(buf.len(), s_1.len());
        buf[..n].copy_from_slice(&s_1[..n]);

        let u = core::cmp::min(buf.len() - n, s_2.len());
        buf[n..(n + u)].copy_from_slice(&s_2[..u]);
        n += u;

        master_in.drain(..n);
        Ok(n)
    }
}

impl Write for Master {
    fn write(&mut self, buf: &[u8]) -> core2::io::Result<usize> {
        let mut discipline = self.discipline.lock().unwrap();
        discipline.process_slave_in(buf, &self.master_in, &self.slave_in);
        Ok(buf.len())
    }

    fn flush(&mut self) -> core2::io::Result<()> {
        todo!("do we flush canonical buffer?");
    }
}

pub struct Slave {
    master_in: Arc<Mutex<VecDeque<u8>>>,
    slave_in: Arc<Mutex<VecDeque<u8>>>,
    discipline: Arc<Mutex<LineDiscipline>>,
}

impl Slave {
    pub fn discipline(&self) -> impl DerefMut<Target = LineDiscipline> + '_ {
        self.discipline.lock().unwrap()
    }
}

impl Read for Slave {
    fn read(&mut self, buf: &mut [u8]) -> core2::io::Result<usize> {
        let mut slave_in = self.slave_in.lock().unwrap();
        let (s_1, s_2) = slave_in.as_slices();

        let mut n = core::cmp::min(buf.len(), s_1.len());
        buf[..n].copy_from_slice(&s_1[..n]);

        let u = core::cmp::min(buf.len() - n, s_2.len());
        buf[n..(n + u)].copy_from_slice(&s_2[..u]);
        n += u;

        slave_in.drain(..n);
        Ok(n)
    }
}

impl Write for Slave {
    fn write(&mut self, buf: &[u8]) -> core2::io::Result<usize> {
        self.master_in.lock().unwrap().extend(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> core2::io::Result<()> {
        Ok(())
    }
}
