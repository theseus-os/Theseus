
use log::*; //{ShutdownLoggerError, SetLoggerError, LogRecord, LogLevel, LogLevelFilter, LogMetadata};

static LOG_LEVEL: LogLevel = LogLevel::Trace;


// TODO: could use this crate: https://github.com/ogham/rust-ansi-term


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
            LogColor::Reset => "\x1b[0m", 
        }
    }
}


struct Logger;

impl ::log::Log for Logger {
    fn enabled(&self, metadata: &LogMetadata) -> bool {
        metadata.level() <= LOG_LEVEL
    }

    fn log(&self, record: &LogRecord) {
        if self.enabled(record.metadata()) {
            let (prefix, color_str) = match record.level() {
                LogLevel::Error => ("E", LogColor::Red.as_terminal_string()),
                LogLevel::Warn => ("W", LogColor::Yellow.as_terminal_string()),
                LogLevel::Info => ("I", LogColor::Cyan.as_terminal_string()),
                LogLevel::Debug => ("D", LogColor::Green.as_terminal_string()),
                LogLevel::Trace => ("T", LogColor::Purple.as_terminal_string()),
            };

            use serial_port;
            serial_port::serial_out( format!("{}[{}] {}{}\n", 
                    color_str, prefix, record.args(), LogColor::Reset.as_terminal_string()).as_str());
        }
    }
}

impl Logger {
    fn flush(&self) {
        // flushing the log is a no-op, since there is no write buffering yet
    }
}



pub fn init_logger() -> Result<(), SetLoggerError> {
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
