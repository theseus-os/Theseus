#![no_std]

use pl011_qemu::{PL011, UART1};
use log::{Record, Metadata, Log, set_logger, set_max_level, STATIC_MAX_LEVEL};
use core::fmt::Write;
use irq_safety::MutexIrqSafe;

type QemuVirtUart = PL011<UART1>;

pub struct Logger {
    pub(crate) uart: MutexIrqSafe<QemuVirtUart>,
}

impl Log for Logger {
    fn enabled(&self, _metadata: &Metadata) -> bool {
        true
    }

    fn log(&self, record: &Record) {
        let mut mutable_uart = self.uart.lock();

        if self.enabled(record.metadata()) {
            let _ = write!(&mut mutable_uart, "{} - {}\r\n", record.level(), record.args());
        }
    }

    fn flush(&self) {}
}

static mut LOGGER: Option<Logger> = None;

pub fn init() -> Result<(), &'static str> {
    set_max_level(STATIC_MAX_LEVEL);

    let uart1 = UART1::take().unwrap();
    let logger = Logger {
        uart: MutexIrqSafe::new(QemuVirtUart::new(uart1)),
    };

    let logger_static = unsafe {
        LOGGER = Some(logger);
        LOGGER.as_ref().unwrap()
    };

    set_logger(logger_static).map_err(|_| "logger::init - couldn't set logger")
}
