//! Simple tests for I/O transfers across the [`serial_port::SerialPort`].
//!

#![no_std]

extern crate alloc;
// #[macro_use] extern crate log;
// #[macro_use] extern crate terminal_print;
extern crate task;
extern crate bare_io;
extern crate serial_port;


use core::{convert::TryFrom};

use alloc::{
    string::String,
    vec::Vec,
};
use bare_io::{Read};
use core::fmt::Write;
use serial_port::{SerialPortAddress, get_serial_port};


pub fn main(args: Vec<String>) -> isize {
    let serial_port_address = args.get(0)
        .and_then(|s| SerialPortAddress::try_from(&**s).ok())
        .unwrap_or(SerialPortAddress::COM1);

    let serial_port = get_serial_port(serial_port_address);

    let mut buf = [0; 100];
    loop {
        let res = serial_port.lock().read(&mut buf);
        if let Ok(bytes_read) = res {
            if let Ok(s) = core::str::from_utf8(&buf[..bytes_read]) {
                serial_port.lock().write_str(s).expect("serial port write failed");
            }
        }
    }
    
    // 0
}
