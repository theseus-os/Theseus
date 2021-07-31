//! A basic logger implementation for system-wide logging in Theseus. 
//!
//! This enables Theseus crates to use the `log` crate's macros anywhere.
//! Currently, log statements are written to one or more serial ports.

#![no_std]

extern crate serial_port;
extern crate log;
extern crate spin;
extern crate irq_safety;

use log::{Record, Level, SetLoggerError, Metadata, Log};
use core::fmt::{self, Write};
use spin::Once;
use serial_port::{SerialPort, SerialPortAddress};
use irq_safety::MutexIrqSafe;


/// The static logger instance. 
/// This is "static" only because it's required by the `log` crate's design.
static LOGGER: Once<Logger> = Once::new();

/// By default, Theseus will print all log levels, including `Trace` and above.
const DEFAULT_LOG_LEVEL: Level = Level::Trace;

pub type LogOutputFunc = fn(fmt::Arguments);
static MIRROR_VGA_FUNC: Once<LogOutputFunc> = Once::new();

/// See ANSI terminal formatting schemes
#[allow(dead_code)]
pub enum LogColor {
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
    pub fn as_terminal_string(&self) -> &'static str {
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
/// Currently, it supports emitting log messages to up to 4 serial ports.
#[derive(Default)]
struct Logger {
    serial_ports: [Option<&'static MutexIrqSafe<SerialPort>>; 4],
}

impl Logger {
    /// Re-implementation of the function from `fmt::Write`, but it doesn't require `&mut self`.
    fn write_fmt(&self, arguments: fmt::Arguments) -> fmt::Result {
        for serial_port in self.serial_ports.iter().flatten() {
            let _result = serial_port.lock().write_fmt(arguments);
            // If there was an error above, there's literally nothing we can do but ignore it,
            // because there is no other lower-level way to log errors than the serial port.
        }
        Ok(())
    }
}

impl Log for Logger {
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
        // flushing the log is a no-op, since there is no write buffering yet
    }
}


/// Initialize the Theseus system logger.
///
/// # Arguments
/// * `log_level`: the log level that should be used.
///    If `None`, the `DEFAULT_LOG_LEVEL` will be used.
/// * `serial_ports`: an iterator over the serial ports that the system logger 
///    will write log messages to.
///    Typically this is just a single port, e.g., `&[COM1]`.
///
/// This function will initialize up to a maximum of 4 serial ports and use them for logging.
/// Serial ports after the first 4 in the `serial_ports` argument will be ignored.
/// 
/// This function also initializes and takes ownership of all specified serial ports
/// such that it can atomically write log messages to them.
pub fn init<'p>(
    log_level: Option<Level>,
    serial_ports: impl IntoIterator<Item = &'p SerialPortAddress>
) -> Result<(), SetLoggerError> {
    let mut logger = Logger::default();
    for (base_port, logger_serial_port) in serial_ports.into_iter().take(4).zip(&mut logger.serial_ports) {
        *logger_serial_port = Some(serial_port::get_serial_port(*base_port));
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
