#![no_std]

use pl011_qemu::PL011;
use log::{Record, Metadata, Log, set_logger, set_max_level, STATIC_MAX_LEVEL};
use core::fmt::Write;
use core::ops::DerefMut;
use irq_safety::MutexIrqSafe;

use memory::{PhysicalAddress, MappedPages, PteFlags, get_kernel_mmi_ref, allocate_pages, allocate_frames_at};

/// This wraps a UART channel handle.
pub struct UartLogger {
    pub(crate) uart: MutexIrqSafe<PL011>,
}

impl Log for UartLogger {
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
static mut LOGGER: Option<UartLogger> = None;

// Mapped Pages for the UART MMIO
static mut UART_MMIO: MappedPages = MappedPages::empty();

/// Initialize the internal global "LOGGER" singleton
/// and sets it as the system-wide logger for the `log`
/// crate.
///
/// Bootstrapping code must call this as early
/// as possible for all log messages to show up
/// on the UART output of Qemu.
pub fn init() -> Result<(), &'static str> {
    set_max_level(STATIC_MAX_LEVEL);

    let kernel_mmi_ref = get_kernel_mmi_ref()
        .ok_or("logger_aarch64: couldn't get kernel MMI ref")?;

    let mut locked = kernel_mmi_ref.lock();
    let page_table = &mut locked.deref_mut().page_table;

    let mmio_flags = PteFlags::DEVICE_MEMORY
                   | PteFlags::NOT_EXECUTABLE
                   | PteFlags::WRITABLE;

    let pages = allocate_pages(1)
        .ok_or("logger_aarch64: couldn't allocate pages for the UART interface")?;

    let qemu_uart_frame = PhysicalAddress::new_canonical(0x0900_0000);
    let frames = allocate_frames_at(qemu_uart_frame, 1)
        .map_err(|_| "logger_aarch64: couldn't allocate frames for the UART interface")?;

    let mapped_pages = page_table.map_allocated_pages_to(pages, frames, mmio_flags)
        .map_err(|_| "logger_aarch64: couldn't map the UART interface")?;

    let addr = mapped_pages.start_address().value();
    let logger = UartLogger {
        uart: MutexIrqSafe::new(PL011::new(addr as *mut _)),
    };

    let logger_static = unsafe {
        UART_MMIO = mapped_pages;
        LOGGER = Some(logger);
        LOGGER.as_ref().unwrap()
    };

    set_logger(logger_static).map_err(|_| "logger_aarch64: couldn't set logger")
}
