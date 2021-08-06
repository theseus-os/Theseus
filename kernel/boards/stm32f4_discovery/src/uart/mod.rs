//! Implements UART specific functionality for the STM32F4 Discovery Board 
use core::{convert::TryFrom, fmt};
use irq_safety::MutexIrqSafe;
use spin::Once;
use stm32f4::stm32f407::{usart1, USART1, USART2, USART3, USART6, RCC, GPIOA};

const USART1_BASE : *const usart1::RegisterBlock = USART1::ptr();
const USART2_BASE : *const usart1::RegisterBlock = USART2::ptr();
const USART3_BASE : *const usart1::RegisterBlock = USART3::ptr();
const USART6_BASE : *const usart1::RegisterBlock = USART6::ptr();

#[derive(Copy, Clone, Debug)]
pub enum SerialPortAddress {
    USART1,
    USART2,
    USART3,
    USART6,
}
impl TryFrom<&str> for SerialPortAddress {
	type Error = ();
	fn try_from(s: &str) -> Result<Self, Self::Error> {
		if s.eq_ignore_ascii_case("USART1") {
			Ok(Self::USART1)
		} else if s.eq_ignore_ascii_case("USART2") {
			Ok(Self::USART2)
		} else if s.eq_ignore_ascii_case("USART3") {
			Ok(Self::USART3)
		} else if s.eq_ignore_ascii_case("USART6") {
			Ok(Self::USART6)
		} else {
			Err(())
		}
	}
}

static USART1_SERIAL_PORT: Once<MutexIrqSafe<SerialPort>> = Once::new();
static USART2_SERIAL_PORT: Once<MutexIrqSafe<SerialPort>> = Once::new();
static USART3_SERIAL_PORT: Once<MutexIrqSafe<SerialPort>> = Once::new();
static USART6_SERIAL_PORT: Once<MutexIrqSafe<SerialPort>> = Once::new();


/// Initialize UART for use.
pub fn uart_init(uart_address: *const usart1::RegisterBlock) {
    unsafe {
        // initializing gpio
        let uart = &*uart_address;
        let gpioa = &*GPIOA::ptr();
        let rcc = &*RCC::ptr();

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
}

pub fn get_serial_port(
    serial_port_address: SerialPortAddress
) -> &'static MutexIrqSafe<SerialPort> {
    let (sp, address_pointer) = match serial_port_address {
        SerialPortAddress::USART1 => (&USART1_SERIAL_PORT, USART1_BASE),
        SerialPortAddress::USART2 => (&USART2_SERIAL_PORT, USART2_BASE),
        SerialPortAddress::USART3 => (&USART3_SERIAL_PORT, USART3_BASE),
        SerialPortAddress::USART6 => (&USART6_SERIAL_PORT, USART6_BASE),
    };
    sp.call_once(|| {
        uart_init(address_pointer);
        MutexIrqSafe::new(SerialPort::new(address_pointer))
    })
}

/// The `SerialPort` struct implements the Write trait that is necessary for use with uprint! macro.
pub struct SerialPort {
    uart_address: *const usart1::RegisterBlock,
} 

unsafe impl Send for SerialPort {}

impl fmt::Write for SerialPort {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        
        unsafe {
            let uart = &*(self.uart_address);
            for byte in s.as_bytes().iter() {
                while uart.sr.read().txe().bit_is_clear() {} 

                uart.dr.write(|w| w.dr().bits(u16::from(*byte)));
            }
        }
        Ok(())
    }
}

impl SerialPort {
    pub fn new(uart_address: *const usart1::RegisterBlock) ->  SerialPort {
        SerialPort {
            uart_address,
        }
    }
}

// /// Macro used to print a formatted string over a serial port on the UART
// #[macro_export]
// macro_rules! uprint {
//     ($serial:expr, $($arg:tt)*) => {
//         $serial.write_fmt(format_args!($($arg)*)).ok()
//     };
// }

// /// Implementation of uprintln! inspired by Rust Embedded Discovery Book
// #[macro_export]
// macro_rules! uprintln {
//     ($serial:expr, $fmt:expr) => {
//         uprint!($serial, concat!($fmt, "\n"))
//     };
//     ($serial:expr, $fmt:expr, $($arg:tt)*) => {
//         uprint!($serial, concat!($fmt, "\n"), $($arg)*)
//     };
// }