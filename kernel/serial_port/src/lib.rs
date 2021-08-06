//! Support for basic serial port access, including initialization, transmit, and receive.
//!
//! This crate provides an abstraction on top of the separate serial port implementations for x86 and ARM, located in `./serial_port_x86` and `./serial_port_arm`.

#![no_std]

#[macro_use]
extern crate cfg_if;

cfg_if ! {
	if #[cfg(target_arch = "x86_64")] {
		extern crate serial_port_x86;
		pub use serial_port_x86::{get_serial_port, SerialPort, SerialPortAddress};
	}

	else if #[cfg(target_arch = "arm")] {
		extern crate serial_port_arm;
		pub use serial_port_arm::{get_serial_port, SerialPort, SerialPortAddress};
	}
}