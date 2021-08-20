//! Implementation of serial ports on arm microcontrollers
//! At the moment, the implementation is simple and does not allow
//! for interrupts or DMA buffering.
//! However, it is sufficient for use with Theseus's logger.
//!
//! When compiling for a specific microcontroller, we distinguish the platform
//! by using the value of `target_vendor` specified by the custom cargo target.
//! When a `target_vendor` is specified, we rely upon the implementations of 
//! `get_serial_port`, `SerialPort`, and `SerialPortAddress` provided by
//! the platform's associated subcrate in `kernel/boards`. This is necessary because
//! each platform has its own peculiarities in working with the UART, so serial port
//! code must be implemented for each platform.
//!
//! When the `target_vendor` is unknown, we rely on a dummy implementation using semihosting,
//! a form of communication that allows a microcontroller to simulate
//! i/o operations on a host device and is supported by most cortex_m CPUs.
//! For more info on semihosting, read (here)[https://www.keil.com/support/man/docs/armcc/armcc_pge1358787046598.htm]
#![no_std]

#[macro_use]
extern crate cfg_if;

cfg_if ! {
    if #[cfg(target_vendor = "stm32f407")] {
        extern crate stm32f4_discovery;
        pub use stm32f4_discovery::uart::{get_serial_port, SerialPort, SerialPortAddress};
    } 

    // Dummy implementation for when no physical device is present, in which case semihosting will be used
    else if #[cfg(all(target_arch = "arm", target_vendor = "unknown"))] {
        extern crate cortex_m_semihosting;
        extern crate irq_safety;
        extern crate spin;

        use cortex_m_semihosting::hio::hstdout;
        use core::fmt::{self, Write};
        use irq_safety::MutexIrqSafe;
        use spin::Once;
        
        #[derive(Copy, Clone, Debug)]
        pub enum SerialPortAddress {
            Semihost,
        }

        static SEMIHOSTING_DUMMY_PORT: Once<MutexIrqSafe<SerialPort>> = Once::new();

        pub fn get_serial_port(
            serial_port_address: SerialPortAddress
        ) -> &'static MutexIrqSafe<SerialPort> {
            let sp = match serial_port_address {
                SerialPortAddress::Semihost => &SEMIHOSTING_DUMMY_PORT,
            };
            sp.call_once(|| MutexIrqSafe::new(SerialPort::new()))
        }

        pub struct SerialPort;

        impl SerialPort {
            pub fn new() -> SerialPort {
                SerialPort
            }
        } 

        impl fmt::Write for SerialPort {
            fn write_str(&mut self, s: &str) -> fmt::Result {
                let mut semihosting_out = hstdout().unwrap();
                semihosting_out.write_all(s.as_bytes()).map_err(|_| fmt::Error)
            }
        }
    }
}
