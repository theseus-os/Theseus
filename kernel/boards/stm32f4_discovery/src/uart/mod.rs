//! Implements UART specific functionality for the STM32F4 Discovery Board 
use crate::{BOARD_GPIOA, BOARD_RCC, BOARD_USART2};
use core::{convert::TryFrom, fmt};
use irq_safety::MutexIrqSafe;
use spin::Once;

#[derive(Copy, Clone, Debug)]
pub enum SerialPortAddress {
//    USART1,
    USART2,
//    USART3,
//    USART6,
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

// static USART1_SERIAL_PORT: Once<MutexIrqSafe<SerialPort>> = Once::new();
static USART2_SERIAL_PORT: Once<MutexIrqSafe<SerialPort>> = Once::new();
// static USART3_SERIAL_PORT: Once<MutexIrqSafe<SerialPort>> = Once::new();
// static USART6_SERIAL_PORT: Once<MutexIrqSafe<SerialPort>> = Once::new();


/// Initialize UART for use.
fn uart_init() {
    let uart_locked = BOARD_USART2.lock();
    let gpioa_locked = BOARD_GPIOA.lock();
    let rcc_locked = BOARD_RCC.lock();
    let uart = uart_locked.borrow();
    let gpioa = gpioa_locked.borrow();
    let rcc = rcc_locked.borrow();

    // initializing clock
    rcc.as_ref().unwrap().ahb1enr.write(|w| w.gpioaen().bit(true));
    // initialize uart clock
    rcc.as_ref().unwrap().apb1enr.write(|w| w.usart2en().bit(true));

    // set up PA2 and PA3 pins for alternate function
    gpioa.as_ref().unwrap().afrl.modify(|_,w| w.afrl2().bits(0b0111).afrl3().bits(0b0111));
    gpioa.as_ref().unwrap().moder.modify(|_,w| w.moder2().bits(0b10).moder3().bits(0b10));

    // configure pin output speeds to high
    gpioa.as_ref().unwrap().ospeedr.modify(|_,w| w.ospeedr2().bits(0b10).ospeedr3().bits(0b10));

    // Enable the USART
    uart.as_ref().unwrap().cr1.modify(|_,w| w.ue().bit(true));

    // Set the word length
    uart.as_ref().unwrap().cr1.modify(|_,w| w.m().bit(false));

    // Program the number of stop bits
    uart.as_ref().unwrap().cr2.modify(|_,w| w.stop().bits(0));

    // Disable DMA transfer
    uart.as_ref().unwrap().cr3.modify(|_,w| w.dmat().bit(false));

    // Select the desired baudrate, in this case 16 MHz / 104.1875 = 9600 bits/second
    uart.as_ref().unwrap().brr.modify(|_,w| w.div_mantissa().bits(104).div_fraction().bits(3));

    // Initialize uart for reading and writing
    uart.as_ref().unwrap().cr1.modify(|_,w| w.te().bit(true).re().bit(true));
}

pub fn get_serial_port(
    serial_port_address: SerialPortAddress
) -> &'static MutexIrqSafe<SerialPort> {
    let sp = match serial_port_address {
        // SerialPortAddress::USART1 => (&USART1_SERIAL_PORT, USART1_BASE),
        SerialPortAddress::USART2 => &USART2_SERIAL_PORT,
        // SerialPortAddress::USART3 => (&USART3_SERIAL_PORT, USART3_BASE),
        // SerialPortAddress::USART6 => (&USART6_SERIAL_PORT, USART6_BASE),
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
        let uart_locked = BOARD_USART2.lock();
        let uart = uart_locked.borrow();
        for byte in s.as_bytes().iter() {
            while uart.as_ref().unwrap().sr.read().txe().bit_is_clear() {} 

            uart.as_ref().unwrap().dr.write(|w| w.dr().bits(u16::from(*byte)));
        }
        Ok(())
    }
}

impl SerialPort {
    pub fn new() ->  SerialPort {
        SerialPort
    }
}
