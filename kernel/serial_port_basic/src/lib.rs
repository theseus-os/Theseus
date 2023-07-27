//! Support for basic serial port access, including initialization, transmit, and receive.
//!
//! This is a near-standalone crate with very minimal dependencies and a basic feature set
//! intended for use during early Theseus boot up and initialization.
//! For a more featureful serial port driver, use the `serial_port` crate.
//!
//! # Notes
//! Some serial port drivers use special cases for transmitting some byte values,
//! specifically `0x08` and `0x7F`, which are ASCII "backspace" and "delete", respectively.
//! They do so by writing them as three distinct values (with proper busy waiting in between):
//! 1. `0x08`
//! 2. `0x20` (an ascii space character)
//! 3. `0x08` again.
//!
//! This isn't necessarily a bad idea, as it "clears out" whatever character was there before,
//! presumably to prevent rendering/display issues for a deleted character. 
//! But, this isn't required, and I personally believe it should be handled by a higher layer,
//! such as a shell or TTY program. 
//! We don't do anything like that here, in case a user of this crate wants to send binary data
//! across the serial port, rather than "smartly-interpreted" ASCII characters.
//!
//! On `x86_64`, this uses I/O ports to access the standard COM1 to COM4 serial ports. On
//! Aarch64 (ARMv8), the system is assumed to present serial ports through the PL011 standard
//! interface. The `arm_boards` crate contains the base addresses for each port.
//!
//! # Resources
//! * <https://en.wikibooks.org/wiki/Serial_Programming/8250_UART_Programming>
//! * <https://tldp.org/HOWTO/Modem-HOWTO-4.html>
//! * <https://wiki.osdev.org/Serial_Ports>
//! * <https://www.sci.muni.cz/docs/pc/serport.txt>

#![no_std]

use sync_irq::IrqSafeMutex;

#[cfg_attr(target_arch = "x86_64", path = "x86_64.rs")]
#[cfg_attr(target_arch = "aarch64", path = "aarch64.rs")]
mod arch;

pub use arch::*;

impl SerialPortAddress {
    /// Returns a reference to the static instance of this serial port.
    fn to_static_port(self) -> &'static IrqSafeMutex<TriState<SerialPort>> {
        match self {
            SerialPortAddress::COM1 => &COM1_SERIAL_PORT,
            SerialPortAddress::COM2 => &COM2_SERIAL_PORT,
            SerialPortAddress::COM3 => &COM3_SERIAL_PORT,
            SerialPortAddress::COM4 => &COM4_SERIAL_PORT,
        }
    }
}

/// This type is used to ensure that an object of type `T` is only initialized once,
/// but still allows for a caller to take ownership of the object `T`.
enum TriState<T> {
    Uninited,
    Inited(T),
    Taken,
}

impl<T> TriState<T> {
    fn take(&mut self) -> Option<T> {
        if let Self::Inited(_) = self {
            if let Self::Inited(v) = core::mem::replace(self, Self::Taken) {
                return Some(v);
            }
        }
        None
    }
}

// Serial ports cannot be reliably probed (discovered dynamically), thus,
// we ensure they are exposed safely as singletons through the below static instances.
static COM1_SERIAL_PORT: IrqSafeMutex<TriState<SerialPort>> = IrqSafeMutex::new(TriState::Uninited);
static COM2_SERIAL_PORT: IrqSafeMutex<TriState<SerialPort>> = IrqSafeMutex::new(TriState::Uninited);
static COM3_SERIAL_PORT: IrqSafeMutex<TriState<SerialPort>> = IrqSafeMutex::new(TriState::Uninited);
static COM4_SERIAL_PORT: IrqSafeMutex<TriState<SerialPort>> = IrqSafeMutex::new(TriState::Uninited);

/// Takes ownership of the [`SerialPort`] specified by the given [`SerialPortAddress`].
///
/// This function initializes the given serial port if it has not yet been initialized.
/// If the serial port has already been initialized and taken by another crate,
/// this returns `None`.
///
/// On aarch64, initializing a serial port includes mapping memory pages; Make sure to have
/// called `memory::init()` ahead of calling this function.
///
/// The returned [`SerialPort`] will be restored to this crate upon being dropped.
pub fn take_serial_port(
    serial_port_address: SerialPortAddress
) -> Option<SerialPort> {
    let sp = serial_port_address.to_static_port();
    let mut locked = sp.lock();
    if let TriState::Uninited = &*locked {
        *locked = TriState::Inited(SerialPort::new(serial_port_address as _));
    }
    locked.take()
}

/// The types of events that can trigger an interrupt on a serial port.
#[derive(Debug)]
#[repr(u8)]
pub enum SerialPortInterruptEvent {
    DataReceived     = 1 << 0,
    TransmitterEmpty = 1 << 1,
    ErrorOrBreak     = 1 << 2,
    StatusChange     = 1 << 3,
}
