#![no_std]
#![feature(alloc)]

#[macro_use] extern crate vga_buffer; // for temp testing on real hardware

extern crate serial_port;
extern crate log;
#[macro_use] extern crate alloc;

use log::*; //{ShutdownLoggerError, SetLoggerError, LogRecord, LogLevel, LogLevelFilter, LogMetadata};

static LOG_LEVEL: LogLevel = LogLevel::Trace;

static mut print_to_vga: bool = false;

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
			LogColor::Black	=> "\x1b[30m",
			LogColor::Red	 => "\x1b[31m",
			LogColor::Green   => "\x1b[32m",
			LogColor::Yellow  => "\x1b[33m",
			LogColor::Blue	=> "\x1b[34m",
			LogColor::Purple  => "\x1b[35m",
            LogColor::Cyan  => "\x1b[36m",
            LogColor::White  => "\x1b[37m",
            LogColor::Reset => "\x1b[0m\n", 
        }
    }
}

/// quick dirty hack to trigger printing to vga once it's set up
pub unsafe fn enable_vga() {
    print_to_vga = true;
}

struct Logger;

impl ::log::Log for Logger {
    fn enabled(&self, metadata: &LogMetadata) -> bool {
        metadata.level() <= LOG_LEVEL
    }

    fn log(&self, record: &LogRecord) {
        if self.enabled(record.metadata()) {
            let (prefix, color_str) = match record.level() {
                LogLevel::Error => ("[E] ", LogColor::Red.as_terminal_string()),
                LogLevel::Warn =>  ("[W] ", LogColor::Yellow.as_terminal_string()),
                LogLevel::Info =>  ("[I] ", LogColor::Cyan.as_terminal_string()),
                LogLevel::Debug => ("[D] ", LogColor::Green.as_terminal_string()),
                LogLevel::Trace => ("[T] ", LogColor::Purple.as_terminal_string()),
            };

            use serial_port;
            let _ = serial_port::write_fmt_log(color_str, prefix, record.args().clone(), LogColor::Reset.as_terminal_string());

            unsafe {
                if print_to_vga {
                    println_unsafe!("{} {}", prefix, record.args().clone());
                }
            }

            // the old way of doing it, which required an allocation unfortunately, 
            // meaning it couldn't be used before the heap was established. Sad!
            // serial_port::serial_out( format!("{}[{}] {}{}\n", 
            //         color_str, prefix, record.args(), LogColor::Reset.as_terminal_string()).as_str());
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
        ::log::set_logger_raw(|max_log_level| {
            static LOGGER: Logger = Logger;
            max_log_level.set(LOG_LEVEL.to_log_level_filter());
            &Logger
        })
    }
}

pub fn shutdown() -> Result<(), ShutdownLoggerError> {
    ::log::shutdown_logger_raw().map(|logger| {
        let logger = unsafe { &*(logger as *const Logger) };
        logger.flush();
    })
}
