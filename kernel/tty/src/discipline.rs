use core::sync::atomic::{AtomicBool, Ordering};

use crate::Channel;
use alloc::{sync::Arc, vec::Vec};
use core2::io::Result;
use mutex_sleep::MutexSleep as Mutex;
use task::{KillReason, TaskRef};

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
/// Modifying a cloned `LineDiscipline` will modify the original:
/// ```rust
/// # use tty::LineDiscipline;
/// let discipline_1 = LineDiscipline::new();
/// assert!(discipline_1.canonical());
/// let discipline_2 = discipline_1.clone();
/// discipline_2.set_canonical(false);
/// assert!(!discipline_1.canonical());
/// ```
///
/// [cfmakeraw]: https://linux.die.net/man/3/cfmakeraw
#[derive(Clone, Debug)]
pub struct LineDiscipline {
    echo: Arc<AtomicBool>,
    /// The input buffer for canonical mode.
    ///
    /// If `None`, canonical mode is disabled
    canonical: Arc<Mutex<Option<Vec<u8>>>>,
    foreground: Arc<Mutex<Option<TaskRef>>>,
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
            echo: Arc::new(AtomicBool::new(true)),
            canonical: Arc::new(Mutex::new(Some(Vec::new()))),
            foreground: Arc::new(Mutex::new(None)),
        }
    }

    /// Resets the line discipline to sane defaults.
    ///
    /// This is equivalent to:
    /// ```rust
    /// # let discipline = tty::LineDiscipline::default();
    /// discipline.set_echo(true);
    /// discipline.set_canonical(true);
    /// ```
    pub fn set_sane(&self) {
        self.set_echo(true);
        self.set_canonical(true);
    }

    /// Sets the line discipline to raw mode.
    ///
    /// This is equivalent to:
    /// ```rust
    /// # let discipline = tty::LineDiscipline::default();
    /// discipline.set_echo(false);
    /// discipline.set_canonical(false);
    /// ```
    pub fn set_raw(&self) {
        self.set_echo(false);
        self.set_canonical(false);
    }

    pub fn echo(&self) -> bool {
        self.echo.load(Ordering::SeqCst)
    }

    /// Sets the echo flag.
    ///
    /// This is equivalent to `ECHO | ECHOE | ECHOCTL` on Linux.
    pub fn set_echo(&self, echo: bool) {
        self.echo.store(echo, Ordering::SeqCst);
    }
    
    pub fn foreground(&self) -> Option<TaskRef> {
        self.foreground.lock().unwrap().clone()
    }

    pub fn set_foreground(&self, foreground: Option<TaskRef>) {
        *self.foreground.lock().unwrap() = foreground;
    }

    pub fn canonical(&self) -> bool {
        self.canonical.lock().unwrap().is_some()
    }

    /// Sets the canonical flag.
    ///
    /// This is equivalent to `ICANON` on Linux.
    pub fn set_canonical(&self, canonical: bool) {
        if canonical {
            *self.canonical.lock().unwrap() = Some(Vec::new());
        } else {
            // TODO: Flush buffer?
            *self.canonical.lock().unwrap() = None;
        }
    }

    fn clear_input_buf(&self) {
        if let Some(ref mut input_buf) = *self.canonical.lock().unwrap() {
            *input_buf = Vec::new();
        }
    }

    pub(crate) fn process_byte(
        &self,
        byte: u8,
        master: &Channel,
        slave: &Channel,
    ) -> Result<()> {
        const ERASE: u8 = 0x7f; // DEL (backspace key)
        const WERASE: u8 = 0x17; // ^W

        const INTERRUPT: u8 = 0x3;
        const SUSPEND: u8 = 0x1a;

        if let Some(ref foreground) = *self.foreground.lock().unwrap() {
            match byte {
                INTERRUPT => {
                    let _ = foreground.kill(KillReason::Requested);
                    self.clear_input_buf();
                    return Ok(());
                }
                SUSPEND => {
                    foreground.suspend();
                    self.clear_input_buf();
                    return Ok(());
                }
                _ => {}
            }
        }

        // TODO: EOF and EOL
        // TODO: UTF-8?
        if self.echo.load(Ordering::SeqCst) {
            match (byte, &*self.canonical.lock().unwrap()) {
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

        if let Some(ref mut input_buf) = *self.canonical.lock().unwrap() {
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
        &self,
        buf: &[u8],
        master: &Channel,
        slave: &Channel,
    ) -> Result<()> {
        for byte in buf {
            // TODO: This locks internal fields on every byte.
            self.process_byte(*byte, master, slave)?;
        }
        Ok(())
    }
}

/// Returns how many characters need to be removed to erase a word.
const fn werase(buf: &[u8]) -> usize {
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
