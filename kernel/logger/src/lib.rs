#![no_std]

extern crate serial_port;
extern crate log;
extern crate spin;

use log::{Record, Level, SetLoggerError, Metadata, Log};
use core::fmt;
use spin::Once;


/// The static logger instance, an empty struct that implements the `Log` trait.
static LOGGER: Logger = Logger { };

/// By default, Theseus will log 
const DEFAULT_LOG_LEVEL: Level = Level::Error;

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

/// A dummy struct that exists so we can implement the Log trait's methods.
struct Logger { }

impl Log for Logger {
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
        let _result = serial_port::write_fmt(format_args!("{}{}{}:{}: {}{}",
            color.as_terminal_string(),
            level_str,
            file_loc,
            line_loc,
            record.args(),
            LogColor::Reset.as_terminal_string(),
        ));
        // If there was an error above, there's literally nothing we can do but ignore it,
        // because there is no other lower-level way to log errors than the serial port.
        
        if let Some(func) = MIRROR_VGA_FUNC.try() {
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


/// Initialize the Theseus system logger, which writes log messages to the serial port. 
pub fn init() -> Result<(), SetLoggerError> {
    log::set_logger(&LOGGER)?;
    set_log_level(DEFAULT_LOG_LEVEL);
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
