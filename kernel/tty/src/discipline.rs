use alloc::{collections::VecDeque, sync::Arc, vec::Vec};
use mutex_sleep::{MutexSleep as Mutex, MutexSleepGuard as MutexGuard};
use vte::Parser;

/// The TTY line discipline.
///
/// The line discipline always converts carriage returns to newlines, equivalent
/// to ICRNL on Linux.
#[derive(Default)]
pub struct LineDiscipline {
    echo: bool,
    /// The input buffer for canonical mode.
    ///
    /// If `None`, canonical mode is disabled
    canonical: Option<Vec<u8>>,
    parser: Parser,
}

impl LineDiscipline {
    /// Sets the echo flag.
    ///
    /// This is equivalent to ECHO + ECHOCTL on Linux.
    pub fn echo(&mut self, echo: bool) {
        self.echo = echo;
    }

    /// Sets the canonical flag.
    ///
    /// This is equivalent to ICANON + IEXTEN.
    pub fn canonical(&mut self, canonical: bool) {
        if canonical {
            self.canonical = Some(Vec::new());
        } else {
            // TODO: Flush buffer?
            self.canonical = None;
        }
    }

    pub(crate) fn process_slave_in(
        &mut self,
        buf: &[u8],
        master_in: &Arc<Mutex<VecDeque<u8>>>,
        slave_in: &Arc<Mutex<VecDeque<u8>>>,
    ) {
        let master_in = master_in.lock().unwrap();
        let slave_in = slave_in.lock().unwrap();
        let discipline = LineDisciplineSettings {
            echo: self.echo,
            canonical: &mut self.canonical,
        };

        let mut performer = Performer {
            master_in,
            slave_in,
            discipline,
        };

        for byte in buf {
            self.parser.advance(&mut performer, *byte);
        }
    }
}

struct Performer<'a, 'b, 'c> {
    master_in: MutexGuard<'a, VecDeque<u8>>,
    slave_in: MutexGuard<'b, VecDeque<u8>>,
    discipline: LineDisciplineSettings<'c>,
}

struct LineDisciplineSettings<'a> {
    echo: bool,
    canonical: &'a mut Option<Vec<u8>>,
}

impl<'a, 'b, 'c> vte::Perform for Performer<'a, 'b, 'c> {
    fn print(&mut self, c: char) {
        let mut bytes = [0; 4];
        let str = c.encode_utf8(&mut bytes);
        let c_bytes = str.as_bytes();

        if self.discipline.echo {
            if let '\r' | '\n' = c {
                self.master_in.push_back(b'\n');
            } else {
                self.master_in.extend(c_bytes);
            }
        }

        if let Some(ref mut input_buf) = self.discipline.canonical {
            // TODO: EOF
            if let '\r' | '\n' = c {
                self.slave_in.extend(core::mem::take(input_buf));
                self.slave_in.push_back(b'\n');
            } else {
                input_buf.extend(c_bytes);
            }
        } else {
            self.slave_in.extend(c_bytes);
        }
    }

    fn execute(&mut self, _byte: u8) {
        panic!("execute");
    }

    fn hook(&mut self, _params: &vte::Params, _intermediates: &[u8], _ignore: bool, _action: char) {
        panic!("hook");
    }

    fn put(&mut self, _byte: u8) {
        panic!("put");
    }

    fn unhook(&mut self) {
        panic!("unhook");
    }

    fn osc_dispatch(&mut self, _params: &[&[u8]], _bell_terminated: bool) {
        panic!("osc_dispatch");
    }

    fn csi_dispatch(
        &mut self,
        _params: &vte::Params,
        _intermediates: &[u8],
        _ignore: bool,
        _action: char,
    ) {
        panic!("csi_dispatch")
    }

    fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, _byte: u8) {
        panic!("esc_dispatch");
    }
}
