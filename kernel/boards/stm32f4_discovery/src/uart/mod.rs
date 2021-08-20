//! Implements UART specific functionality for the STM32F4 Discovery Board 
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

/// Exposes the board's USART2
pub static BOARD_USART2: Once<MutexIrqSafe<stm32f407::USART2>> = Once::new();

#[derive(Copy, Clone, Debug)]
pub enum SerialPortAddress {
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

static USART2_SERIAL_PORT: Once<MutexIrqSafe<SerialPort>> = Once::new();


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

pub fn get_serial_port(
    serial_port_address: SerialPortAddress
) -> &'static MutexIrqSafe<SerialPort> {
    let sp = match serial_port_address {
        SerialPortAddress::USART2 => &USART2_SERIAL_PORT,
    };
    sp.call_once(|| {
        uart_init();
        MutexIrqSafe::new(SerialPort::new())
    })
}

/// The `SerialPort` struct implements the Write trait that is necessary for use with uprint! macro.
pub struct SerialPort;

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

impl SerialPort {
    pub fn new() ->  SerialPort {
        SerialPort
    }
}
