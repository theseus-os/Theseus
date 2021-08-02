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
//! 2. `0x20` (an ascii space character)
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

extern crate spin;
extern crate port_io;
extern crate irq_safety;
extern crate bare_io;

use core::{convert::TryFrom, fmt};
use port_io::Port;
use irq_safety::MutexIrqSafe;
use spin::Once;


/// The base port I/O addresses for COM serial ports.
#[derive(Copy, Clone, Debug)]
#[repr(u16)]
pub enum SerialPortAddress {
	/// The base port I/O address for the COM1 serial port.
	COM1 = 0x3F8,
	/// The base port I/O address for the COM2 serial port.
	COM2 = 0x2F8,
	/// The base port I/O address for the COM3 serial port.
	COM3 = 0x3E8,
	/// The base port I/O address for the COM4 serial port.
	COM4 = 0x2E8,
}
impl TryFrom<&str> for SerialPortAddress {
	type Error = ();
	fn try_from(s: &str) -> Result<Self, Self::Error> {
		if s.eq_ignore_ascii_case("COM1") {
			Ok(Self::COM1)
		} else if s.eq_ignore_ascii_case("COM2") {
			Ok(Self::COM2)
		} else if s.eq_ignore_ascii_case("COM3") {
			Ok(Self::COM3)
		} else if s.eq_ignore_ascii_case("COM4") {
			Ok(Self::COM4)
		} else {
			Err(())
		}
	}
}

static COM1_SERIAL_PORT: Once<MutexIrqSafe<SerialPort>> = Once::new();
static COM2_SERIAL_PORT: Once<MutexIrqSafe<SerialPort>> = Once::new();
static COM3_SERIAL_PORT: Once<MutexIrqSafe<SerialPort>> = Once::new();
static COM4_SERIAL_PORT: Once<MutexIrqSafe<SerialPort>> = Once::new();


/// Obtains a reference to the [`SerialPort`] specified by the given [`SerialPortAddress`].
///
/// This function initializes the given serial port if it has not yet been initialized. 
pub fn get_serial_port(
	serial_port_address: SerialPortAddress
) -> &'static MutexIrqSafe<SerialPort> {
	let sp = match serial_port_address {
		SerialPortAddress::COM1 => &COM1_SERIAL_PORT,
		SerialPortAddress::COM2 => &COM2_SERIAL_PORT,
		SerialPortAddress::COM3 => &COM3_SERIAL_PORT,
		SerialPortAddress::COM4 => &COM4_SERIAL_PORT,
	};
	sp.call_once(|| MutexIrqSafe::new(SerialPort::new(serial_port_address as u16)))
}


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
	/// * `base_port`: the port number (port I/O address) of the serial port. 
	///    This should generally be one of the known serial ports, e.g., on x86, 
	///    [`SerialPortAddress::COM1`] through [`SerialPortAddress::COM4`].
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

	/// Write the given string to the serial port, blocking until data can be transmitted.
	///
	/// # Special characters
	/// Because this function writes strings, it will transmit a carriage return `'\r'`
	/// after transmitting a line feed (new line) `'\n'` to ensure a proper new line.
	pub fn out_str(&mut self, s: &str) {
		for byte in s.bytes() {
			self.out_byte(byte);
			if byte == b'\n' {
				self.out_byte(b'\r');
			} else if byte == b'\r' {
				self.out_byte(b'\n');
			}
		}
	}

	/// Write the given byte to the serial port, blocking until data can be transmitted.
	///
	/// This writes the byte directly with no special cases, e.g., new lines.
	pub fn out_byte(&mut self, byte: u8) {
		while !self.ready_to_transmit() { }

		// SAFE: we're just writing to the serial port, which has already been initialized.
		unsafe { 
			self.data.write(byte); 
			// E9.write(byte); // for Bochs debugging
		}
	}

	/// Write the given bytes to the serial port, blocking until data can be transmitted.
	///
	/// This writes the bytes directly with no special cases, e.g., new lines.
	pub fn out_bytes(&mut self, bytes: &[u8]) {
		for byte in bytes {
			self.out_byte(*byte);
		}
	}

	/// Read one byte from the serial port, blocking until data is available.
	pub fn in_byte(&mut self) -> u8 {
		while !self.data_available() { }
		self.data.read() 
	}

	/// Reads multiple bytes from the serial port into the given `buffer`, non-blocking.
	///
	/// The buffer will be filled with as many bytes as are available in the serial port.
	/// Once data is no longer available to be read, the read operation will stop. 
	///
	/// If no data is immediately available on the serial port, this will read nothing and return `0`.
	///
	/// Returns the number of bytes read into the given `buffer`.
	pub fn in_bytes(&mut self, buffer: &mut [u8]) -> usize {
		let mut bytes_read = 0;
		for byte in buffer {
			if !self.data_available() {
				break;
			}
			*byte = self.data.read();
			bytes_read += 1;
		}
		bytes_read
	}

	/// Returns `true` if the serial port is ready to transmit a byte.
	#[inline(always)]
	pub fn ready_to_transmit(&self) -> bool {
		self.line_status.read() & 0x20 == 0x20
	}

	/// Return `true` if the serial port has data available to read.
	#[allow(unused)]
	#[inline(always)]
	pub fn data_available(&self) -> bool {
		self.line_status.read() & 0x01 == 0x01
	}
}

impl fmt::Write for SerialPort {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.out_str(s); 
        Ok(())
    }
}

/// A non-blocking implementation of `Read` that will read bytes into the given `buf`
/// so long as more bytes are available.
/// The read operation will be completed when there are no more bytes to be read,
/// or when the `buf` is filled, whichever comes first.
///
/// Because it's non-blocking, a [`bare_io::ErrorKind::WouldBlock`] error is returned
/// if there are no bytes available to be read, indicating that the read would block.
impl bare_io::Read for SerialPort {
	fn read(&mut self, buf: &mut [u8]) -> bare_io::Result<usize> {
		if !self.data_available() {
			return Err(bare_io::ErrorKind::WouldBlock.into());
		}
        Ok(self.in_bytes(buf))
    }
}

/// A blocking implementation of `Write` that will write bytes from the given `buf`
/// to the `SerialPort`, waiting until it is ready to transfer all bytes. 
///
/// The `flush()` function is a no-op, since the `SerialPort` does not have buffering. 
impl bare_io::Write for SerialPort {
    fn write(&mut self, buf: &[u8]) -> bare_io::Result<usize> {
		self.out_bytes(buf);
		Ok(buf.len())
    }

    fn flush(&mut self) -> bare_io::Result<()> {
        Ok(())
    }    
}
