use crate::Channel;
use alloc::vec::Vec;

/// The TTY line discipline.
///
/// The line discipline always converts carriage returns to newlines, equivalent
/// to ICRNL on Linux.
pub struct LineDiscipline {
    echo: bool,
    /// The input buffer for canonical mode.
    ///
    /// If `None`, canonical mode is disabled
    canonical: Option<Vec<u8>>,
}

impl Default for LineDiscipline {
    fn default() -> Self {
        Self {
            echo: true,
            canonical: Some(Vec::new()),
        }
    }
}

impl LineDiscipline {
    /// Sets the echo flag.
    ///
    /// This is equivalent to ECHO + ECHOE + ECHOCTL on Linux.
    pub fn echo(&mut self, echo: bool) {
        self.echo = echo;
    }

    /// Sets the canonical flag.
    ///
    /// This is equivalent to ICANON.
    pub fn canonical(&mut self, canonical: bool) {
        if canonical {
            self.canonical = Some(Vec::new());
        } else {
            // TODO: Flush buffer?
            self.canonical = None;
        }
    }

    pub(crate) fn process_slave_in(&mut self, buf: &[u8], master: &Channel, slave: &Channel) {
        const ERASE: u8 = 0x7f; // DEL (backspace key)
        const WERASE: u8 = 0x17; // ^W

        for byte in buf {
            // TODO: UTF-8?
            if self.echo {
                match (*byte, &self.canonical) {
                    (b'\r', _) => master.send_buf([b'\r', b'\n']),
                    // TODO: Also pass-through START and STOP characters
                    (b'\t' | b'\n', _) => master.send(*byte),
                    (ERASE, Some(input_buf)) => {
                        if !input_buf.is_empty() {
                            master.send_buf([0x8, b' ', 0x8])
                        }
                    }
                    (WERASE, Some(input_buf)) => {
                        if !input_buf.is_empty() {
                            // TODO: Cache offset. Currently we're calculating it twice.
                            let offset = werase(input_buf);
                            for _ in 0..offset {
                                master.send_buf([0x8, b' ', 0x8])
                            }
                        }
                    }
                    (0..=0x1f, _) => master.send_buf([b'^', byte + 0x40]),
                    _ => master.send(*byte),
                }
            }

            if let Some(ref mut input_buf) = self.canonical {
                match *byte {
                    b'\r' | b'\n' => {
                        slave.send_buf(core::mem::take(input_buf));
                        slave.send(b'\n');
                    }
                    ERASE => {
                        input_buf.pop();
                    }
                    WERASE => {
                        for _ in 0..werase(input_buf) {
                            input_buf.pop();
                        }
                    }
                    _ => input_buf.push(*byte),
                }
            } else {
                slave.send(*byte);
            }
        }
    }
}

/// Returns how many characters need to be removed to erase a word.
fn werase(buf: &[u8]) -> usize {
    let len = buf.len();
    let mut offset = 0;

    let mut initial_whitespace = true;

    // TODO: Tabs?

    loop {
        if offset == len {
            return offset;
        }

        offset += 1;

        if initial_whitespace {
            if buf[len - offset] != b' ' {
                initial_whitespace = false;
            }
        } else if buf[len - offset] == b' ' {
            return offset - 1;
        }
    }
}
