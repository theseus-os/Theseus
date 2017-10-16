#![no_std]
#![feature(const_fn)]

extern crate port_io;

use port_io::Port;

const SERIAL_PORT_COM1: u16 = 0x3F8;
const SERIAL_PORT_COM1_READY: u16 = SERIAL_PORT_COM1 + 5;
const SERIAL_PORT_READY_MASK: u8 = 0x20;


static COM1: Port<u8> = Port::new(SERIAL_PORT_COM1);
static COM1_READY: Port<u8> = Port::new(SERIAL_PORT_COM1_READY);


const PORT_E9: u16 = 0xE9; // for use with bochs
static E9: Port<u8> = Port::new(PORT_E9); // see Bochs's port E9 hack


pub fn serial_out(s: &str) {
	for b in s.bytes() {
		serial_outb(b);
	}
}

pub fn serial_outb(b: u8) {
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


// a hack wrapper for using the serial port with fmt::Write
static mut SERIAL_PORT: SerialPort = SerialPort { };
struct SerialPort; 

use core::fmt;
use core::fmt::Write;

pub fn write_fmt(args: fmt::Arguments) -> ::core::fmt::Result {
	write_fmt_log("", "", args, "")
}

/// convenience function for use with the Logger
pub fn write_fmt_log(prefix1: &str, prefix2: &str, args: fmt::Arguments, suffix: &str) -> ::core::fmt::Result {
	serial_out(prefix1);
	serial_out(prefix2);

	// SAFE: as above, worst-case effect is just out-of-order characters in the serial log.
	let ret = unsafe {
		SERIAL_PORT.write_fmt(args)
	};

	serial_out(suffix);
	ret
}

impl fmt::Write for SerialPort {
    fn write_str(&mut self, s: &str) -> ::core::fmt::Result {
        serial_out(s); 
        Ok(())
    }
}