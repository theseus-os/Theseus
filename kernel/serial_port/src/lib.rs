//! Support for basic serial port access, including initialization, transmit, and receive.
//!
//! Resources used for this implementation:
//! * <https://en.wikibooks.org/wiki/Serial_Programming/8250_UART_Programming>
//! * <https://tldp.org/HOWTO/Modem-HOWTO-4.html>
//! * <https://wiki.osdev.org/Serial_Ports>
//!
//! # Notes
//! Some serial port drivers use special cases for transmitting some byte values,
//! specifically `0x08` and `0x7F`, which are ASCII "backspace" and "delete", respectively.
//! They do so by writing them as three distinct values (with proper busy waiting in between):
//! 1. `0x08`
//! 2. `0x20`  (an ascii space character)
//! 3. `0x08` again. 
//!
//! This isn't necessarily a bad idea, as it "clears out" whatever character was there before,
//! presumably to prevent rendering/display issues for a deleted character. 
//! But, this isn't required, and I personally believe it should be handled by a higher layer,
//! such as a shell or TTY program. 
//! We don't do anything like that here, in case a user of this crate wants to send binary data
//! across the serial port, rather than "smartly-interpreted" ASCII characters.
//! 

#![no_std]
#![feature(const_fn)]

extern crate port_io;

use core::fmt;
use port_io::Port;

pub const COM1_BASE_PORT: u16 = 0x3F8;
pub const COM2_BASE_PORT: u16 = 0x2F8;
pub const COM3_BASE_PORT: u16 = 0x3E8;
pub const COM4_BASE_PORT: u16 = 0x2E8;

// The E9 port can be used with the Bochs emulator for extra debugging info.
// const PORT_E9: u16 = 0xE9; // for use with bochs
// static E9: Port<u8> = Port::new(PORT_E9); // see Bochs's port E9 hack


/// A serial port and its various data and control registers.
///
/// TODO: use PortReadOnly and PortWriteOnly to set permissions for each register.
pub struct SerialPort {
    data:                       Port<u8>,
    interrupt_enable:           Port<u8>,
    interrupt_id_fifo_control:  Port<u8>,
    line_control:               Port<u8>,
    modem_control:              Port<u8>,
    line_status:                Port<u8>,
    _modem_status:              Port<u8>,
    _scratch:                   Port<u8>,
}

impl SerialPort {
	/// Creates and returns a new serial port structure, 
	/// and initializes that port using standard configuration parameters. 
	/// 
	/// The configuration parameters used in this function are:
	/// * A baud rate of 38400.
	/// * "8N1" mode: data word length of 8 bits, with no parity and one stop bit.
	/// * FIFO buffer enabled with a threshold of 14 bytes.
	/// * Interrupts enabled for receiving bytes only (not transmitting).
	///
	/// # Arguments
	/// * `base_port`: the number (port I/O "address") of the serial port. 
	///    This should generally be one of the known serial ports, e.g., on x86, 
	///    `COM1_BASE_PORT`, `COM2_BASE_PORT`, `COM3_BASE_PORT`, `COM4_BASE_PORT`.
	///
	/// Note: if you are experiencing problems with serial port behavior,
	/// try enabling the loopback test part of this function to see if that passes.
	pub fn new(base_port: u16) -> SerialPort {
		let serial = SerialPort {
			data:                       Port::new(base_port + 0),
			interrupt_enable:           Port::new(base_port + 1),
			interrupt_id_fifo_control:  Port::new(base_port + 2),
			line_control:               Port::new(base_port + 3),
			modem_control:              Port::new(base_port + 4),
			line_status:                Port::new(base_port + 5),
			_modem_status:              Port::new(base_port + 6),
			_scratch:                   Port::new(base_port + 7),
		};

		// SAFE: we are just accessing this serial port's registers.
		unsafe {
			// Before doing anything, disable interrupts for this serial port.
			serial.interrupt_enable.write(0x00);

			// Enter DLAB mode so we can set the baud rate divisor
			serial.line_control.write(0x80);
			// Set baud rate to 38400, which requires a divisor value of `3`. 
			// To do this, we enter DLAB mode (to se the baud rate divisor),
			// the write the low byte of the divisor to the data register (DLL)
			// and the high byte to the interrupt enable register (DLH).
			serial.data.write(0x03);
			serial.interrupt_enable.write(0x00);

			// Exit DLAB mode. At the same time, set the data word length to 8 bits,
			// also specifying no parity and one stop bit. This is known as "8N1" mode.
			serial.line_control.write(0x03);

			// Enable the FIFO queues (buffers in hardware) and clear both the transmit and receive queues.
			// Also, set an interrupt threshold of 14 (0xC) bytes, which is the maximum value.
			// Note that serial ports will fire an interrupt if there is a "small delay"
			// between bytes, so we don't always have to wait for 14 entire bytes to arrive.
			serial.interrupt_id_fifo_control.write(0xC7);

			// Mark the data terminal as ready, signal request to send
			// and enable auxilliary output #2 (used as interrupt line for CPU)
			serial.modem_control.write(0x0B);

			// Below, we can optionally test the serial port to see if the chip is working. 
			let _test_passed = if false {
				const TEST_BYTE: u8 = 0xAE;
				// Enable "loopback" mode (set bit 4), write a byte to the data port and try to read it back.
				serial.modem_control.write(0x10 | (TEST_BYTE & 0x0F));
				serial.data.write(TEST_BYTE);
				let byte_read_back = serial.data.read();
				byte_read_back == TEST_BYTE
			} else {
				true
			};
			
			// Note: even if the above loopback test failed, we go ahead and ensure the serial port
			// remains in a working state, because some hardware doesn't support loopback mode. 
			
			// Set the serial prot to regular mode (non-loopback) and enable standard config bits:
			// Auxiliary Output 1 and 2, Request to Send (RTS), and Data Terminal Ready (DTR).
			serial.modem_control.write(0x0F);
			
			// Finally, enable interrupts for this serial port, for received data only.
			serial.interrupt_enable.write(0x01);
		}

		serial
	}

	/// Write the given string to the serial port. 
	pub fn out_str(&mut self, s: &str) {
		for b in s.bytes() {
			self.out_byte(b);
		}
	}

	/// Write the given byte to the serial port.
	pub fn out_byte(&mut self, b: u8) {
		self.wait_until_ready_to_transmit();

		// SAFE because we're just writing to the serial port. 
		// worst-case effects here are simple out-of-order characters in the serial log.
		unsafe { 
			self.data.write(b); 
			// E9.write(b); // for Bochs debugging
		}
	}

	/// Blocks until the serial port is ready to transmit another byte.
	#[inline(always)]
	fn wait_until_ready_to_transmit(&self) {
		while self.line_status.read() & 0x20 == 0 {
			// do nothing
		}
	}

	/// Blocks until the serial port has received a byte.
	#[allow(unused)]
	#[inline(always)]
	fn wait_until_data_received(&self) {
		while self.line_status.read() & 0x01 == 0 {
			// do nothing
		}
	}
}

impl fmt::Write for SerialPort {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.out_str(s); 
        Ok(())
    }
}
