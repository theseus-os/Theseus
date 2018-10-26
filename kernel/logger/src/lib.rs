#![no_std]

extern crate serial_port;
extern crate log;
extern crate spin;

use log::{LogRecord, LogLevel, SetLoggerError, LogMetadata, Log, ShutdownLoggerError};
use core::fmt;
use spin::Once;

static LOG_LEVEL: LogLevel = LogLevel::Trace;

pub type LogOutputFunc = fn(&LogColor, &'static str, fmt::Arguments);
static MIRROR_VGA_FUNC:     Once<LogOutputFunc> = Once::new();
static MIRROR_NETWORK_FUNC: Once<LogOutputFunc> = Once::new();

/// See ANSI terminal formatting schemes
#[allow(dead_code)]
pub enum LogColor {
    Reset,
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Purple,
    Cyan,
    White,
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

/// Call this to enable mirroring logging macros to the network
pub fn mirror_to_network(func: LogOutputFunc) {
    MIRROR_VGA_FUNC.call_once(|| func);
}

/// A dummy struct that exists so we can implement the Log trait's methods.
struct Logger { }

impl Log for Logger {
    fn enabled(&self, metadata: &LogMetadata) -> bool {
        metadata.level() <= LOG_LEVEL
    }

    fn log(&self, record: &LogRecord) {
        if !self.enabled(record.metadata()) {
            return;
        }

        let (prefix, color) = match record.level() {
            LogLevel::Error => ("[E] ", LogColor::Red),
            LogLevel::Warn =>  ("[W] ", LogColor::Yellow),
            LogLevel::Info =>  ("[I] ", LogColor::Cyan),
            LogLevel::Debug => ("[D] ", LogColor::Green),
            LogLevel::Trace => ("[T] ", LogColor::Purple),
        };

        use serial_port;
        let _ = serial_port::write_fmt_log(color.as_terminal_string(), prefix, record.args().clone(), LogColor::Reset.as_terminal_string());

        
        if let Some(func) = MIRROR_VGA_FUNC.try() {
            func(&color, prefix, record.args().clone());
        }

        if let Some(func) = MIRROR_NETWORK_FUNC.try() {
            func(&color, prefix, record.args().clone());
        }
    }
}


impl Logger {
    fn flush(&self) {
        // flushing the log is a no-op, since there is no write buffering yet
    }
}



pub fn init() -> Result<(), SetLoggerError> {
    unsafe {
        log::set_logger_raw(|max_log_level| {
            static LOGGER: Logger = Logger { };
            max_log_level.set(LOG_LEVEL.to_log_level_filter());
            &LOGGER
        })
    }
}

pub fn shutdown() -> Result<(), ShutdownLoggerError> {
    log::shutdown_logger_raw().map(|logger| {
        let logger = unsafe { &*(logger as *const Logger) };
        logger.flush();
    })
}
