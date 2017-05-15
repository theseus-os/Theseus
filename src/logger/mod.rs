
use log::*; //{ShutdownLoggerError, SetLoggerError, LogRecord, LogLevel, LogLevelFilter, LogMetadata};

static LOG_LEVEL: LogLevel = LogLevel::Debug;

struct Logger;

impl ::log::Log for Logger {
    fn enabled(&self, metadata: &LogMetadata) -> bool {
        metadata.level() <= LOG_LEVEL
    }

    fn log(&self, record: &LogRecord) {
        if self.enabled(record.metadata()) {
            println!("[{}] {}", record.level(), record.args());
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