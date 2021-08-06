#![no_std]

#[macro_use]
extern crate cfg_if;

cfg_if ! {
    if #[cfg(target_vendor = "stm32f407")] {
        extern crate stm32f4_discovery;
        pub use stm32f4_discovery::uart::{get_serial_port, SerialPort, SerialPortAddress};
    } 

    // Dummy implementation for when no physical device is present, in which case semihosting will be used
    else if #[cfg(target_vendor = "unknown")] {
        extern crate cortex_m_semihosting;
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
