#![no_std]
#![feature(alloc)]

extern crate serial_port;
extern crate log;
extern crate spin;
extern crate network;
#[macro_use] extern crate alloc;

use log::*; //{ShutdownLoggerError, SetLoggerError, LogRecord, LogLevel, LogLevelFilter, LogMetadata};
use core::fmt;
use spin::Once;
use network::server::UDP_TEST_SERVER;
use alloc::*;
use alloc::string::ToString;


static LOG_LEVEL: LogLevel = LogLevel::Debug;
static MIRROR_VGA_FUNC: Once<fn(LogColor, &'static str, fmt::Arguments)> = Once::new();


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

/// Call this to mirror logging macros to the VGA text buffer
pub fn mirror_to_vga(func: fn(LogColor, &'static str, fmt::Arguments)) {
    MIRROR_VGA_FUNC.call_once(|| func);
}

struct Logger;

impl ::log::Log for Logger {
    fn enabled(&self, metadata: &LogMetadata) -> bool {
        metadata.level() <= LOG_LEVEL
    }

    fn log(&self, record: &LogRecord) {
        if self.enabled(record.metadata()) {
            let (prefix, color) = match record.level() {
                LogLevel::Error => ("[E] ", LogColor::Red),
                LogLevel::Warn =>  ("[W] ", LogColor::Yellow),
                LogLevel::Info =>  ("[I] ", LogColor::Cyan),
                LogLevel::Debug => ("[D] ", LogColor::Green),
                LogLevel::Trace => ("[T] ", LogColor::Purple),
            };

            use serial_port;
            let _ = serial_port::write_fmt_log(color.as_terminal_string(), prefix, record.args().clone(), LogColor::Reset.as_terminal_string());


            // Copying the serial port messages to the UDP server to forward them through UDP
            if let Some(producer) = UDP_TEST_SERVER.try(){
                let s = format!("{}", record.args().clone());
                let mut len: usize;
                // if s.len() > 128 {
                //     len = 128;
                // }
                // else {
                //     len = s.len();
                // }
                
                len = s.len();
                producer.enqueue(s[0..len].to_string());           

            }
            
            
            if let Some(func) = MIRROR_VGA_FUNC.try() {
                func(color, prefix, record.args().clone());
                
            }
            

            
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
            &LOGGER
        })
    }
}

pub fn shutdown() -> Result<(), ShutdownLoggerError> {
    ::log::shutdown_logger_raw().map(|logger| {
        let logger = unsafe { &*(logger as *const Logger) };
        logger.flush();
    })
}
