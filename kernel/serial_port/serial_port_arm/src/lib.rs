#![no_std]

#[macro_use]
extern crate cfg_if;

cfg_if ! {
    if #[cfg(target_vendor = "stm32f407")] {
        extern crate stm32f4_discovery;
        pub use stm32f4_discovery::uart::{get_serial_port, SerialPort, SerialPortAddress};
    }
}
