//! A basic logger implementation for system-wide logging in Theseus. 
//!
//! This enables Theseus crates to use the `log` crate's macros anywhere,
//! such as `error!()`, `warn!()`, `info!()`, `debug!()`, and `trace!()`.
//!
//! Currently, log statements are written to one or more **writers`, 
//! which are objects that implement the [`core::fmt::Write`] trait.

#![no_std]

extern crate log;
extern crate spin;
extern crate irq_safety;

use log::{Record, Level, SetLoggerError, Metadata, Log};
use core::fmt::{self, Write};
use spin::Once;
use irq_safety::MutexIrqSafe;

/// The singleton system-wide logger instance. 
///
/// This is "static" only because it's required by the `log` crate.
static LOGGER: Once<Logger<LOG_MAX_WRITERS>> = Once::new();

/// By default, Theseus will print all log levels, including `Trace` and above.
pub const DEFAULT_LOG_LEVEL: Level = Level::Trace;

/// The maximum number of writers: backends to which log streams can be outputted.
pub const LOG_MAX_WRITERS: usize = 2;

/// The signature of a callback function that will optionally be invoked
/// on every log statement to be printed, which enables log mirroring.
/// See [`mirror_to_vga()`].
pub type LogOutputFunc = fn(fmt::Arguments);
static MIRROR_VGA_FUNC: Once<LogOutputFunc> = Once::new();

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

/// Call this to enable mirroring logging macros to the screen
pub fn mirror_to_vga(func: LogOutputFunc) {
    MIRROR_VGA_FUNC.call_once(|| func);
}

/// A struct that holds information about logging destinations in Theseus.
/// 
/// This is the "backend" for the `log` crate that allows Theseus to use its `log!()` macros.
///
/// We force the use of static references here to enable this crate to be used
/// before dynamic heap allocation has been set up.
struct Logger<const N: usize> {
    writers: [Option<&'static MutexIrqSafe<dyn Write + Send>>; N],
}
impl<const N: usize> Default for Logger<{N}> {
    fn default() -> Self {
        Logger { writers: [None; N] }
    }
}

impl<const N: usize> Logger<{N}> {
    /// Re-implementation of the function from `fmt::Write`, but it doesn't require `&mut self`.
    fn write_fmt(&self, arguments: fmt::Arguments) -> fmt::Result {
        for writer in self.writers.iter().flatten() {
            let _result = writer.lock().write_fmt(arguments);
            // If there was an error above, there's literally nothing we can do but ignore it,
            // because there is no other lower-level way to log errors than this logger.
        }
        Ok(())
    }
}

impl<const N: usize> Log for Logger<{N}> {
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
        
        if let Some(func) = MIRROR_VGA_FUNC.get() {
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


/// Initialize the Theseus system logger.
///
/// # Arguments
/// * `log_level`: the log level that should be used.
///    If `None`, the [`DEFAULT_LOG_LEVEL`] will be used.
/// * `writers`: an iterator over the backends that the system logger 
///    will write log messages to.
///    Typically this is just a single writer, such as the COM1 serial port.
///
/// This function will initialize the logger with a maximum of [`LOG_MAX_WRITERS`] writers;
/// any additional writers in the given `writers` iterator will be ignored. 
///
/// This function accepts only static references to log writers in order to
/// enable loggers to be used before dynamic heap allocation has been set up.
pub fn init<'i, W: Write + Send + 'static>(
    log_level: Option<Level>,
    writers: impl IntoIterator<Item = &'i &'static MutexIrqSafe<W>>,
) -> Result<(), SetLoggerError> {
    let mut logger = Logger::default();
    for (writer, logger_writer) in writers.into_iter().take(LOG_MAX_WRITERS).zip(&mut logger.writers) {
        *logger_writer = Some(*writer);
    } 

    let static_logger = LOGGER.call_once(|| logger);
    log::set_logger(static_logger)?;
    set_log_level(log_level.unwrap_or(DEFAULT_LOG_LEVEL));
    Ok(())
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
    LOGGER.get()
        .ok_or(fmt::Error)
        .and_then(|logger| logger.write_fmt(args))
}

/// Convenience function for writing a simple string to the logger.
///
/// If the logger has not yet been initialized, no log messages will be emitted.
/// and an `Error` will be returned.
pub fn write_str(s: &str) -> fmt::Result {
    crate::write_fmt(format_args!("{}", s))
}
