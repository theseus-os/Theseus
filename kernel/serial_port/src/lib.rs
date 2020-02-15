#![no_std]
#![feature(const_fn)]

extern crate port_io;
extern crate irq_safety;

use port_io::Port;
use irq_safety::MutexIrqSafe;

const SERIAL_PORT_COM1: u16 = 0x3F8;
const SERIAL_PORT_COM1_READY: u16 = SERIAL_PORT_COM1 + 5;
const SERIAL_PORT_READY_MASK: u8 = 0x20;

static COM1: Port<u8> = Port::new(SERIAL_PORT_COM1);
static COM1_READY: Port<u8> = Port::new(SERIAL_PORT_COM1_READY);

const PORT_E9: u16 = 0xE9; // for use with bochs
static E9: Port<u8> = Port::new(PORT_E9); // see Bochs's port E9 hack

/// The singleton serial port instance for COM1, 
/// which enforces that log messages are printed atomically.
static SERIAL_PORT: MutexIrqSafe<SerialPort> = MutexIrqSafe::new(SerialPort { });

/// An empty type wrapper for using the serial port with `fmt::Write`.
struct SerialPort; 

impl SerialPort {
	/// Write the given string to the serial port. 
	fn out_str(&mut self, s: &str) {
		for b in s.bytes() {
			self.out_byte(b);
		}
	}

	/// Write the given byte to the serial port.
	fn out_byte(&mut self, b: u8) {
		self.wait_for_ready();

		// SAFE because we're just writing to the serial port. 
		// worst-case effects here are simple out-of-order characters in the serial log.
		unsafe { 
			COM1.write(b); 
			E9.write(b);
		}
	}

	/// Blocks until the serial port is ready to transfer another byte.
	fn wait_for_ready(&self) {
		while COM1_READY.read() & SERIAL_PORT_READY_MASK == 0 {
			// do nothing
		}
	}
}

use core::fmt;
use core::fmt::Write;

impl fmt::Write for SerialPort {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.out_str(s); 
        Ok(())
    }
}

/// Write formatted arguments to the COM1 serial port.
/// 
/// Use the `format_args!()` macro from the core library to create
/// the `Arguments` parameter needed here.
pub fn write_fmt(args: fmt::Arguments) -> fmt::Result {
	let mut serial = SERIAL_PORT.lock();
	serial.write_fmt(args)
}

/// Write the given string to the COM1 serial port.
pub fn write_str(s: &str) -> fmt::Result {
	let mut serial = SERIAL_PORT.lock();
	serial.write_str(s)
}

#[inline(never)]
pub fn write_test(s: &str) {
	write_str(s).unwrap()
}
