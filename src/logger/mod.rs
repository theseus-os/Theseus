extern crate log;

use log::{LogRecord, LogLevel, LogMetadata};

static DEFAULT_LOG_LEVEL: LogLevelFilter::

struct Logger;

impl log::Log for Logger {
    fn enabled(&self, metadata: &LogMetadata) -> bool {
        metadata.level() <= LogLevel::Info
    }

    fn log(&self, record: &LogRecord) {
        if self.enabled(record.metadata()) {
            println!("{} - {}", record.level(), record.args());
        }
    }
}



pub fn init_logger() -> Result<(), SetLoggerError> {
    unsafe {
        log::set_logger_raw(|max_log_level| {
            static LOGGER: Logger = Logger;
            max_log_level.set(LogLevelFilter::Info);
            &Logger
        })
    }
}
pub fn shutdown() -> Result<(), ShutdownLoggerError> {
    log::shutdown_logger_raw().map(|logger| {
        let logger = unsafe { &*(logger as *const Logger) };
        logger.flush();
    })
}