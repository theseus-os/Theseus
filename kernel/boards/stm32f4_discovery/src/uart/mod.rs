//! Implements UART specific functionality for the STM32F4 Discovery board.
use crate::{
    gpio::BOARD_GPIOA, 
    rcc::BOARD_RCC,
};
use core::{
    convert::TryFrom, 
    fmt,
};
use irq_safety::MutexIrqSafe;
use spin::Once;
use stm32f4::stm32f407;

/// Exposes the board's USART2.
pub static BOARD_USART2: Once<MutexIrqSafe<stm32f407::USART2>> = Once::new();

/// All available UART addresses.
/// Note: Although the board technically supports traditional UARTs,
/// it is better to utilize the board's USARTs, as they support a higher
/// data transfer rate.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SerialPortAddress {
    /// The STM32F407 USART2, which has a TX pin at pin PA2 and an RX pin at pin PA3.
    USART2,
}
impl TryFrom<&str> for SerialPortAddress {
	type Error = ();
	fn try_from(s: &str) -> Result<Self, Self::Error> {
		if s.eq_ignore_ascii_case("USART2") {
			Ok(Self::USART2)
		} else {
			Err(())
		}
	}
}
impl SerialPortAddress {
    /// Returns a reference to a static instance of this serial port.
    fn to_static_port(&self) -> &'static MutexIrqSafe<TriState<SerialPort>> {
        match self {
            SerialPortAddress::USART2 => &USART2_SERIAL_PORT,
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
static USART2_SERIAL_PORT: MutexIrqSafe<TriState<SerialPort>> = MutexIrqSafe::new(TriState::Uninited);

/// Takes ownership of the [`SerialPort`] specified by the given [`SerialPortAddress`].
///
/// This function initializes the given serial port if it has not yet been initialized.
/// If the serial port has already been initialized and taken by another crate,
/// this returns `None`.
///
/// The returned [`SerialPort`] will be restored to this crate upon being dropped.
pub fn take_serial_port(
    serial_port_address: SerialPortAddress
) -> Option<SerialPort> {
    let sp = serial_port_address.to_static_port();
    let mut locked = sp.lock();
    if let TriState::Uninited = &*locked {
        *locked = TriState::Inited(SerialPort::new());
    }
    locked.take()
}


/// Initialize UART for use.
fn uart_init() {
    let uart = BOARD_USART2.get().unwrap().lock();
    let gpioa = BOARD_GPIOA.get().unwrap().lock();
    let rcc = BOARD_RCC.get().unwrap().lock();

    // initializing clock
    rcc.ahb1enr.write(|w| w.gpioaen().bit(true));
    // initialize uart clock
    rcc.apb1enr.write(|w| w.usart2en().bit(true));

    // set up PA2 and PA3 pins for alternate function
    gpioa.afrl.modify(|_,w| w.afrl2().bits(0b0111).afrl3().bits(0b0111));
    gpioa.moder.modify(|_,w| w.moder2().bits(0b10).moder3().bits(0b10));

    // configure pin output speeds to high
    gpioa.ospeedr.modify(|_,w| w.ospeedr2().bits(0b10).ospeedr3().bits(0b10));

    // Enable the USART
    uart.cr1.modify(|_,w| w.ue().bit(true));

    // Set the word length
    uart.cr1.modify(|_,w| w.m().bit(false));

    // Program the number of stop bits
    uart.cr2.modify(|_,w| w.stop().bits(0));

    // Disable DMA transfer
    uart.cr3.modify(|_,w| w.dmat().bit(false));

    // Select the desired baudrate, in this case 16 MHz / 104.1875 = 9600 bits/second
    uart.brr.modify(|_,w| w.div_mantissa().bits(104).div_fraction().bits(3));

    // Initialize uart for reading and writing
    uart.cr1.modify(|_,w| w.te().bit(true).re().bit(true));
}

/// The [`SerialPort`] struct implements the `Write` trait for use with logging capabilities.
pub struct SerialPort;

impl SerialPort {
    pub fn new() ->  SerialPort {
        uart_init();
        SerialPort
    }
}

impl Drop for SerialPort {
    fn drop(&mut self) {
        let sp = SerialPortAddress::USART2.to_static_port();
        let mut sp_locked = sp.lock();
        if let TriState::Taken = &*sp_locked {
            let dummy = SerialPort;
            let dropped = core::mem::replace(self, dummy);
            *sp_locked = TriState::Inited(dropped);
        }
    }
}


impl fmt::Write for SerialPort {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let uart = BOARD_USART2.get().unwrap().lock();
        for byte in s.as_bytes().iter() {
            while uart.sr.read().txe().bit_is_clear() {} 

            uart.dr.write(|w| w.dr().bits(u16::from(*byte)));
        }
        Ok(())
    }
}
