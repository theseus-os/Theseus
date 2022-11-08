#![no_std]

use pl011_qemu::{PL011, UART1};
use log::{Record, Metadata, Log, set_logger, set_max_level, STATIC_MAX_LEVEL};
use core::{fmt::Write, mem::MaybeUninit};

type QemuVirtUart = PL011<UART1>;

pub struct Logger {
    pub pl011: PL011<UART1>,
}

impl Log for Logger {
    fn enabled(&self, _metadata: &Metadata) -> bool {
        true
    }

    fn log(&self, record: &Record) {
        let mutable = unsafe { (self as *const Self).cast_mut().as_mut().unwrap() };

        if self.enabled(record.metadata()) {
            let _ = write!(&mut mutable.pl011, "{} - {}\r\n", record.level(), record.args());
        }
    }

    fn flush(&self) {}
}

pub static mut LOGGER: Logger = unsafe { MaybeUninit::uninit().assume_init() };

pub fn init() {
    unsafe {
        LOGGER = Logger { pl011: QemuVirtUart::new(UART1::take().unwrap()) };
        set_logger(&LOGGER).unwrap();
        set_max_level(STATIC_MAX_LEVEL);
    }
}
