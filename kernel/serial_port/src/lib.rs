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


fn serial_out(s: &str) {
	for b in s.bytes() {
		serial_outb(b);
	}
}

 fn serial_outb(b: u8) {
	wait_for_ready();

	// SAFE because we're just writing to the serial port. 
	// worst-case effects here are simple out-of-order characters in the serial log.
	unsafe { 
		COM1.write(b); 
		E9.write(b);
	}
}


fn wait_for_ready() {
	while COM1_READY.read() & SERIAL_PORT_READY_MASK == 0 {
		// do nothing
	}
}


use core::fmt;
use core::fmt::Write;

/// An empty type for using the serial port with fmt::Write
struct SerialPort; 

/// A wrapper enforcing that log messages are printed atomically
static SERIAL_PORT: MutexIrqSafe<SerialPort> = MutexIrqSafe::new(SerialPort { });


impl fmt::Write for SerialPort {
    fn write_str(&mut self, s: &str) -> ::core::fmt::Result {
        serial_out(s); 
        Ok(())
    }
}

/// Write formatted arguments to the serial port.
pub fn write_fmt(args: fmt::Arguments) -> ::core::fmt::Result {
	let mut serial = SERIAL_PORT.lock();
	serial.write_fmt(args)
}

/// Write a str reference to the serial port.
pub fn write_str(s: &str) -> ::core::fmt::Result {
	let mut serial = SERIAL_PORT.lock();
	serial.write_str(s)
}

/// Write a formatted log message with two prefixed and one suffix to the given formatted arguments.
/// A convenience function for use with the Logger.
pub fn write_fmt_log(prefix1: &str, prefix2: &str, args: fmt::Arguments, suffix: &str) -> ::core::fmt::Result {
	let mut serial = SERIAL_PORT.lock();

	serial_out(prefix1);
	serial_out(prefix2);

	let ret = serial.write_fmt(args); 

	serial_out(suffix);
	ret
}
