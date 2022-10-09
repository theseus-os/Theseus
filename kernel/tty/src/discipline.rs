use crate::Channel;
use alloc::vec::Vec;
use core2::io::Result;

// FIXME: Ctrl+C, Ctrl+Z, etc.

/// A TTY line discipline.
///
/// The line discipline can be configured based on what application is using the
/// slave end. Most applications should use the [`sane`](Self::sane) setting,
/// which handles line editing and echoing to the terminal. Applications that
/// require more control over the display should use the [`raw`](Self::raw)
/// setting.
///
/// The line discipline's behaviour is documented in terms of Linux `termios`
/// flags. For more information, visit the [`cfmakeraw`
/// documentation][cfmakeraw].
///
/// When the line discipline encounters a carriage return and echoing is
/// enabled, it will send a carriage return followed by a line feed to the
/// master. If canonical mode is enabled, it will convert the carriage return to
/// a line feed (hence flushing the input buffer). This behaviour is equivalent
/// to `ICRNL` on Linux.
///
/// [cfmakeraw]: https://linux.die.net/man/3/cfmakeraw
pub struct LineDiscipline {
    echo: bool,
    /// The input buffer for canonical mode.
    ///
    /// If `None`, canonical mode is disabled
    canonical: Option<Vec<u8>>,
}

impl Default for LineDiscipline {
    /// Equivalent to [`Self::new`].
    fn default() -> Self {
        Self::new()
    }
}

impl LineDiscipline {
    /// Creates a new line discipline with sane defaults.
    pub fn new() -> Self {
        Self {
            echo: true,
            canonical: Some(Vec::new()),
        }
    }

    /// Resets the line discipline to sane defaults.
    ///
    /// This is equivalent to:
    /// ```rust
    /// # let discipline = Self::default();
    /// discipline.echo(true);
    /// discipline.canonical(true);
    /// ```
    pub fn sane(&mut self) {
        self.echo(true);
        self.canonical(true);
    }

    /// Sets the line discipline to raw mode.
    ///
    /// This is equivalent to:
    /// ```rust
    /// # let discipline = Self::default();
    /// discipline.echo(false);
    /// discipline.canonical(false);
    /// ```
    pub fn raw(&mut self) {
        self.echo(false);
        self.canonical(false);
    }

    /// Sets the echo flag.
    ///
    /// This is equivalent to `ECHO | ECHOE | ECHOCTL` on Linux.
    pub fn echo(&mut self, echo: bool) {
        self.echo = echo;
    }

    /// Sets the canonical flag.
    ///
    /// This is equivalent to `ICANON` on Linux.
    pub fn canonical(&mut self, canonical: bool) {
        if canonical {
            self.canonical = Some(Vec::new());
        } else {
            // TODO: Flush buffer?
            self.canonical = None;
        }
    }

    pub(crate) fn process_byte(
        &mut self,
        byte: u8,
        master: &Channel,
        slave: &Channel,
    ) -> Result<()> {
        const ERASE: u8 = 0x7f; // DEL (backspace key)
        const WERASE: u8 = 0x17; // ^W

        // TODO: EOF and EOL
        // TODO: UTF-8?
        if self.echo {
            match (byte, &self.canonical) {
                (b'\r', _) => {
                    master.send_buf([b'\r', b'\n'])?;
                }
                // TODO: Also pass-through START and STOP characters
                (b'\t' | b'\n', _) => {
                    master.send(byte)?;
                }
                (ERASE, Some(input_buf)) => {
                    if !input_buf.is_empty() {
                        master.send_buf([0x8, b' ', 0x8])?
                    }
                }
                (WERASE, Some(input_buf)) => {
                    if !input_buf.is_empty() {
                        // TODO: Cache offset. Currently we're calculating it twice: once here,
                        // and once if canonical mode is enabled.
                        let offset = werase(input_buf);
                        for _ in 0..offset {
                            master.send_buf([0x8, b' ', 0x8])?;
                        }
                    }
                }
                (0..=0x1f, _) => {
                    master.send_buf([b'^', byte + 0x40])?;
                }
                _ => {
                    master.send(byte)?;
                }
            }
        }

        if let Some(ref mut input_buf) = self.canonical {
            match byte {
                b'\r' | b'\n' => {
                    slave.send_buf(core::mem::take(input_buf))?;
                    slave.send(b'\n')?;
                }
                ERASE => {
                    input_buf.pop();
                }
                WERASE => {
                    for _ in 0..werase(input_buf) {
                        input_buf.pop();
                    }
                }
                _ => input_buf.push(byte),
            }
        } else {
            slave.send(byte)?;
        }
        Ok(())
    }

    pub(crate) fn process_buf(
        &mut self,
        buf: &[u8],
        master: &Channel,
        slave: &Channel,
    ) -> Result<()> {
        for byte in buf {
            self.process_byte(*byte, master, slave)?;
        }
        Ok(())
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
