//! Implements UART specific functionality for the STM32F4 Discovery Board 
use core::fmt;
use crate::STM_PERIPHERALS;

/// Initialize UART for use.
pub fn uart_init() {
    {
        let p = STM_PERIPHERALS.lock();
        let uart = &p.USART2;

        // initializing gpio
        let gpioa = &p.GPIOA;
        let rcc = &p.RCC;

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

/// Macro used to print a formatted string over a serial port on the UART
#[macro_export]
macro_rules! uprint {
    ($serial:expr, $($arg:tt)*) => {
        $serial.write_fmt(format_args!($($arg)*)).ok()
    };
}

/// Implementation of uprintln! inspired by Rust Embedded Discovery Book
#[macro_export]
macro_rules! uprintln {
    ($serial:expr, $fmt:expr) => {
        uprint!($serial, concat!($fmt, "\n"))
    };
    ($serial:expr, $fmt:expr, $($arg:tt)*) => {
        uprint!($serial, concat!($fmt, "\n"), $($arg)*)
    };
}

/// The `SerialPort` struct implements the Write trait that is necessary for use with uprint! macro.
pub struct SerialPort; 

impl fmt::Write for SerialPort {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        {
            let p = STM_PERIPHERALS.lock();
            let usart = &p.USART2;
            for byte in s.as_bytes().iter() {
                while usart.sr.read().txe().bit_is_clear() {} 

                usart.dr.write(|w| w.dr().bits(u16::from(*byte)));
            }
        }
        Ok(())
    }
}

impl SerialPort {
    pub fn get_uart() ->  SerialPort {
        SerialPort
    }
}