//! A basic logger implementation for system-wide logging in Theseus. 
//!
//! This enables Theseus crates to use the [`log`] crate's macros anywhere,
//! such as `error!()`, `warn!()`, `info!()`, `debug!()`, and `trace!()`.
//!
//! Currently, log statements are written to one or more **writers**, 
//! which are objects that implement the [`core::fmt::Write`] trait.
//!
//! Early log messages (before memory management is initialized) are saved
//! to a static fixed-sized buffer such that they are not lost and
//! can be retrieved once logging sinks are ready to be used.

#![no_std]
#![feature(trait_alias)]

extern crate alloc;
extern crate crossbeam_utils;
extern crate log;
extern crate sync_irq;
extern crate serial_port_basic;

use log::{Record, Level, Metadata, Log};
use core::{borrow::Borrow, fmt::{self, Write}, ops::Deref};
use sync_irq::IrqSafeMutex;
use serial_port_basic::SerialPort;
use alloc::{sync::Arc, vec::Vec};

#[cfg(mirror_log_to_vga)]
pub use mirror_log::set_log_mirror_function;

/// By default, Theseus will print all log levels, including `Trace` and above.
pub const DEFAULT_LOG_LEVEL: Level = Level::Trace;

/// The maximum number of output streams that a logger can write to.
pub const LOG_MAX_WRITERS: usize = 2;

/// The size of the buffer used to save early log messages.
pub const EARLY_LOG_BUFFER_SIZE: usize = {
    #[cfg(target_arch = "x86_64")]  { 0 }
    #[cfg(target_arch = "aarch64")] { 16 * 1024 }
};

/// The early logger used before dynamic heap allocation is available.
static EARLY_LOGGER: IrqSafeMutex<EarlyLogger> = IrqSafeMutex::new(EarlyLogger::new());

/// The early log buffer.
///
/// This is separate from the `EARLY_LOGGER` in order for it to be placed
/// in `.bss` instead of `.data`, saving space in the executable.
static EARLY_LOG_BUFFER: IrqSafeMutex<EarlyLogBuffer<EARLY_LOG_BUFFER_SIZE>> =
    IrqSafeMutex::new(EarlyLogBuffer::new());

/// The real logger instance where log states are kept.
///
/// This is accessed in the [`DummyLogger`]'s log/write methods,
/// it is not called directly by the `log` crate.
/// If `None`, it is uninitialized, and the [`EARLY_LOGGER`] will be used as a fallback.
static LOGGER: IrqSafeMutex<Option<Logger>> = IrqSafeMutex::new(None);

/// An early logger that can only write to a fixed number of [`SerialPort`]s,
/// intended for basic use before dynamic heap allocation is available.
struct EarlyLogger([Option<SerialPort>; LOG_MAX_WRITERS]);
impl EarlyLogger {
    const fn new() -> Self {
        const INIT: Option<SerialPort> = None;
        Self([INIT; LOG_MAX_WRITERS])
    }

    /// Initializes this early logger with the given serial port writers.
    ///
    /// Flushes the early log buffer to the newly-added serial ports, if any.
    fn init(&mut self, serial_ports: impl IntoIterator<Item = SerialPort>) {
        let buffer_was_truncated: bool;
        {
            let mut buffer = EARLY_LOG_BUFFER.lock();
            buffer_was_truncated = buffer.truncated;

            let mut added_new_loggers = false;
            for (mut sp, logger_writer) in serial_ports.into_iter()
                .take(LOG_MAX_WRITERS)
                .zip(&mut self.0)
            {
                sp.out_bytes(buffer.get_buf());
                *logger_writer = Some(sp);
                added_new_loggers = true;
            }

            if added_new_loggers {
                buffer.truncate(0);
            }
        }

        if buffer_was_truncated {
            let _ = write!(
                self,
                "\n\n{} \
                ---- (early log was truncated; try increasing logger::EARLY_LOG_BUFFER_SIZE) ----\
                \n{}",
                LogColor::Yellow.as_terminal_string(),
                LogColor::Reset.as_terminal_string(),
            );
        }
    }
}
impl fmt::Write for EarlyLogger {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let mut written = false;
        for serial_port in self.0.iter_mut().flatten() {
            let _ = serial_port.write_str(s);
            written = true;
        }
        if !written {
            let _ = EARLY_LOG_BUFFER.lock().write_str(s);
        }
        Ok(())
    }
}

/// The fully-featured logger that can be dynamically initialized with arbitrary output streams.
///
/// This is the "backend" for the `log` crate that allows Theseus to use its `log!()` macros.
struct Logger {
    writers: Vec<Arc<IrqSafeMutex<dyn Write + Send>>>,
}

/// Removes all of the writers (output streams) from the early logger and returns them.
///
/// This is intended to allow the caller to take ownership of the early logger writers
/// such that they can switch to initializing the full logger.
pub fn take_early_log_writers() -> [Option<SerialPort>; LOG_MAX_WRITERS] {
    let mut list = [None, None];
    for (opt, ret) in EARLY_LOGGER.lock().0.iter_mut().zip(&mut list) {
        *ret = opt.take();
    }
    list
}

/// A statically-sized buffer for storing early log messages
/// before memory management is initialized.
struct EarlyLogBuffer<const SIZE: usize> {
    array: [u8; SIZE],
    length: usize,
    truncated: bool,
}

impl<const SIZE: usize> EarlyLogBuffer<SIZE> {
    const fn new() -> Self {
        Self {
            array: [0; SIZE],
            length: 0,
            truncated: false,
        }
    }

    /// Shortens this buffer to the given new `length` in bytes.
    ///
    /// If the new `length` exceeds the current buffer length, this does nothing.
    pub fn truncate(&mut self, length: usize) {
        if length <= self.length {
            self.length = length;
            self.truncated = true;
        }
    }

    fn get_buf(&self) -> &[u8] {
        &self.array[0..self.length]
    }
}

impl<const SIZE: usize> fmt::Write for EarlyLogBuffer<SIZE> {
    /// Appends the given string to the buffer.
    ///
    /// Returns an error if there is insufficient space in the buffer.
    ///
    /// Upon a failed write, the caller can truncate the buffer
    /// to its last correct length using [`EarlyLogBuffer::truncate()`].
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let new_length = self.length + s.len();
        if let Some(arr) = self.array.get_mut(self.length .. new_length) {
            arr.copy_from_slice(s.as_bytes());
            self.length = new_length;
            Ok(())
        } else {
            self.length = SIZE;
            self.truncated = true;
            Err(fmt::Error)
        }
    }
}


/// The static instance of the dummy logger, as required by the `log` crate.
static DUMMY_LOGGER: DummyLogger = DummyLogger;

/// An empty logger struct used to satisfy the requirements of the `log` crate.
///
/// This exists because the `log` crate only allows a logger implementation
/// to be initialized once from a singleton static instance.
/// To get around that limitation, we store the actual logger states
/// **outside** of the logger struct, such that we can modify them later 
/// after the `log` crate has already been initialized.
struct DummyLogger;

impl DummyLogger {
    /// A re-implementation of [`core::fmt::Write::write_fmt()`]
    /// that doesn't require `&mut self`.
    ///
    /// This function writes to the real (fully-featured) [`LOGGER`] if it has been initialized;
    /// otherwise, it falls back to writing to the [`EARLY_LOGGER`] instead.
    fn write_fmt(&self, arguments: fmt::Arguments) -> fmt::Result {
        if let Some(logger) = &*LOGGER.lock() {
            for writer in logger.writers.iter() {
                let _ = writer.deref().borrow().lock().write_fmt(arguments);
            }
        } else {
            let _ = EARLY_LOGGER.lock().write_fmt(arguments);
        }
        // If there was an error above, there's literally nothing we can do but ignore it,
        // because there is no other lower-level way to log errors than this logger.
        Ok(())
    }
}

impl Log for DummyLogger {
    #[inline(always)]
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= log::max_level()
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }

        let (level_str, color) = match record.level() {
            Level::Error => ("[E] ", LogColor::Red),
            Level::Warn =>  ("[W] ", LogColor::Yellow),
            Level::Info =>  ("[I] ", LogColor::Cyan),
            Level::Debug => ("[D] ", LogColor::Green),
            Level::Trace => ("[T] ", LogColor::Purple),
        };
        let file_loc = record.file().unwrap_or("??");
        let line_loc = record.line().unwrap_or(0);
        let _result = self.write_fmt(
            format_args!("{}{}{}:{}: {}{}",
                color.as_terminal_string(),
                level_str,
                file_loc,
                line_loc,
                record.args(),
                LogColor::Reset.as_terminal_string(),
            )
        );
        // If there was an error above, there's literally nothing we can do but ignore it,
        // because there is no other lower-level way to log errors than the serial port.
        
        #[cfg(mirror_log_to_vga)]
        if let Some(func) = mirror_log::get_log_mirror_function() {
            // Currently printing to the VGA terminal doesn't support ANSI color escape sequences,
            // so we exclude the first and the last elements that set those colors.
            func(format_args!("{}{}:{}: {}",
                level_str,
                file_loc,
                line_loc,
                record.args(),
            ));
        }
    }

    fn flush(&self) {
        // flushing the log is a no-op, since there is no write buffering.
    }
}


/// Initializes Theseus's early system logger for use before memory management is set up.
///
/// The early logger can save log messages to a statically-sized buffer
/// and can also write log messages to a list of basic serial ports.
///
/// # Arguments
/// * `log_level`: the log level that should be used.
///    If `None`, the [`DEFAULT_LOG_LEVEL`] will be used.
/// * `serial_ports`: an iterator of [`SerialPort`]s that the logger will write log messages to.
///    Typically this is just a single serial port, e.g., `COM1`.
///
/// This function will initialize the logger with a maximum of [`LOG_MAX_WRITERS`] serial ports;
/// any additional ones in the given iterator beyond that will be ignored.
pub fn early_init(
    log_level: Option<Level>,
    serial_ports: impl IntoIterator<Item = SerialPort>,
) {
    // Initialize the early logger. This can be called multiple times,
    // and will flush the saved early log buffer to the new `serial_ports` each time.
    EARLY_LOGGER.lock().init(serial_ports);

    // Tell the `log` crate to use our early logger via the dummy logger.
    // We ignore any errors returned to allow `early_init` to be called multiple times.
    let _ = log::set_logger(&DUMMY_LOGGER); 
    set_log_level(log_level.unwrap_or(DEFAULT_LOG_LEVEL));
}


/// Initialize the fully-featured Theseus system logger.
///
/// # Arguments
/// * `log_level`: the log level that should be used.
///    If `None`, the [`DEFAULT_LOG_LEVEL`] will be used.
/// * `writers`: an iterator over the backends that the system logger 
///    will write log messages to.
///    Typically this is just a single writer, such as the COM1 serial port.
pub fn init<I, W>(log_level: Option<Level>, writers: impl IntoIterator<Item = I>)
where
    W: Write + Send + 'static,
    I: Into<Arc<IrqSafeMutex<W>>>,
{
    // Populate the fields of the real logger instance
    let logger = Logger {
        writers: writers.into_iter()
            .map(|i| i.into() as Arc<IrqSafeMutex<dyn Write + Send>>)
            .collect::<Vec<_>>(),
    };
    *LOGGER.lock() = Some(logger);

    // Once the real logger has been initialized, tell the `log` crate to use our dummy logger instance.
    // Call `set_logger()` again, just in case we never ran the `early_init()` function;
    // if `early_init()` has already been called, `set_logger()` will return an Error, which is okay.
    let _ = log::set_logger(&DUMMY_LOGGER);
    set_log_level(log_level.unwrap_or(DEFAULT_LOG_LEVEL));
}

/// Set the log level, which determines whether a given log message is actually logged. 
/// 
/// For example, if `Level::Trace` is set, all log levels will be logged.
/// 
/// If `Level::Info` is set, `debug!()` and `trace!()` will not be logged, 
/// but `info!()`, `warn!()`, and `error!()` will be. 
pub fn set_log_level(level: Level) {
    log::set_max_level(level.to_level_filter())
}

/// Convenience function for writing formatted arguments to the logger.
///
/// If the logger has not yet been initialized, no log messages will be emitted
/// and an `Error` will be returned.
/// 
/// Tip: use the `format_args!()` macro from the core library to create
/// the `Arguments` parameter needed here.
pub fn write_fmt(args: fmt::Arguments) -> fmt::Result {
    DUMMY_LOGGER.write_fmt(args)
}

/// Convenience function for writing a simple string to the logger.
///
/// If the logger has not yet been initialized, no log messages will be emitted.
/// and an `Error` will be returned.
pub fn write_str(s: &str) -> fmt::Result {
    crate::write_fmt(format_args!("{s}"))
}


/// ANSI style codes for basic colors.
#[allow(dead_code)]
enum LogColor {
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Purple,
    Cyan,
    White,
    Reset,
}
impl LogColor {
    fn as_terminal_string(&self) -> &'static str {
        match *self {
            // \x1b is the ESC character (0x1B)
			LogColor::Black	  =>  "\x1b[30m",
			LogColor::Red	  =>  "\x1b[31m",
			LogColor::Green   =>  "\x1b[32m",
			LogColor::Yellow  =>  "\x1b[33m",
			LogColor::Blue	  =>  "\x1b[34m",
			LogColor::Purple  =>  "\x1b[35m",
            LogColor::Cyan    =>  "\x1b[36m",
            LogColor::White   =>  "\x1b[37m",
            LogColor::Reset   =>  "\x1b[0m\n", 
        }
    }
}

#[cfg(mirror_log_to_vga)]
mod mirror_log {
    use core::fmt;
    use crossbeam_utils::atomic::AtomicCell;

    /// Call this to enable mirroring of logger output (e.g., via logging macros)
    /// to another output sink, such as the VGA screen.
    pub fn set_log_mirror_function(func: fn(fmt::Arguments)) {
        MIRROR_LOG_FUNC.store(Some(func));
    }

    pub(crate) fn get_log_mirror_function() -> Option<fn(fmt::Arguments)> {
        MIRROR_LOG_FUNC.load()
    }

    /// The callback function that will optionally be invoked
    /// on every log statement to be printed, which enables log mirroring.
    pub(crate) static MIRROR_LOG_FUNC: AtomicCell<Option<fn(fmt::Arguments)>> = AtomicCell::new(None);
    const _: () = assert!(AtomicCell::<fn(fmt::Arguments)>::is_lock_free());
}
