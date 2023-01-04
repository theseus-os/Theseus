#![no_std]

use pl011_qemu::{PL011, UART1};
use log::{Record, Metadata, Log, set_logger, set_max_level, STATIC_MAX_LEVEL};
use core::fmt::Write;
use irq_safety::MutexIrqSafe;

type QemuVirtUart = PL011<UART1>;

/// This wraps a UART channel handle.
pub struct QemuVirtUartLogger {
    pub(crate) uart: MutexIrqSafe<QemuVirtUart>,
}

impl Log for QemuVirtUartLogger {
    fn enabled(&self, _metadata: &Metadata) -> bool {
        // allow all messages
        true
    }

    fn log(&self, record: &Record) {
        let mut mutable_uart = self.uart.lock();

        if self.enabled(record.metadata()) {
            // result is discarded because we
            // have no alternative way to signal
            // an issue to the user
            let _ = write!(&mut mutable_uart, "{} - {}\r\n", record.level(), record.args());
        }
    }

    fn flush(&self) {}
}

// Global logger Singleton
static mut LOGGER: Option<QemuVirtUartLogger> = None;

/// Initialize the internal global "LOGGER" singleton
/// and sets it as the system-wide logger for the `log`
/// crate.
///
/// Bootstrapping code must call this as early
/// as possible for all log messages to show up
/// on the UART output of Qemu.
pub fn init() -> Result<(), &'static str> {
    set_max_level(STATIC_MAX_LEVEL);

    let uart1 = UART1::take().unwrap();
    let logger = QemuVirtUartLogger {
        uart: MutexIrqSafe::new(QemuVirtUart::new(uart1)),
    };

    let logger_static = unsafe {
        LOGGER = Some(logger);
        LOGGER.as_ref().unwrap()
    };

    set_logger(logger_static).map_err(|_| "logger::init - couldn't set logger")
}
