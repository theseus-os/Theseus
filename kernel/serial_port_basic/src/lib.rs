//! A full serial driver with more advanced I/O support, e.g., interrupt-based data receival.
//!
//! This crate provides an abstraction on top of the separate serial port implementations for x86 and ARM, located in `./serial_port_x86` and `./serial_port_arm`.

#![no_std]

#[macro_use]
extern crate cfg_if;

cfg_if ! {
	if #[cfg(target_arch = "x86_64")] {
		extern crate serial_port_basic_x86;
		pub use serial_port_basic_x86::{
            take_serial_port, 
            SerialPort, 
            SerialPortAddress,
        };
	}

	else if #[cfg(target_arch = "arm")] {
		extern crate serial_port_basic_arm;
		pub use serial_port_basic_arm::{
			take_serial_port, 
			SerialPort, 
			SerialPortAddress
		};
	}
}
