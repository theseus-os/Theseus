//! Simple tests for I/O transfers across the [`serial_port::SerialPort`].
//!

#![no_std]

extern crate alloc;
// #[macro_use] extern crate log;
#[macro_use] extern crate app_io;
extern crate task;
extern crate core2;
extern crate io;
extern crate sync_irq;
extern crate serial_port;


use core::convert::TryFrom;
use alloc::{
    string::String,
    vec::Vec,
};
use core2::io::Read;
use io::LockableIo;
use sync_irq::IrqSafeMutex;
use serial_port::{SerialPort, SerialPortAddress, get_serial_port};


pub fn main(args: Vec<String>) -> isize {
    let serial_port_address = args.get(0)
        .and_then(|s| SerialPortAddress::try_from(&**s).ok())
        .unwrap_or(SerialPortAddress::COM1);

    let serial_port = match get_serial_port(serial_port_address) {
        Some(sp) => sp.clone(),
        _ => {
            println!("Error: serial port {:?} was not initialized.", serial_port_address);
            return -1;
        }
    };
    
    if true {
        let mut serial_port_io = LockableIo::<SerialPort, IrqSafeMutex<_>, _>::from(serial_port);
        let mut serial_port_io2 = serial_port_io.clone();
        let mut buf = [0; 100];
        loop {
            let res = serial_port_io2.read(&mut buf);
            if let Ok(bytes_read) = res {
                if let Ok(s) = core::str::from_utf8(&buf[..bytes_read]) {
                    use core2::io::Write;
                    serial_port_io.write(s.as_bytes()).expect("serial port write failed");
                }
            }
        }
    }
    else {
        let mut buf = [0; 100];
        loop {
            let res = serial_port.lock().read(&mut buf);
            if let Ok(bytes_read) = res {
                if let Ok(s) = core::str::from_utf8(&buf[..bytes_read]) {
                    use core::fmt::Write;
                    serial_port.lock().write_str(s).expect("serial port write failed");
                }
            }
        }
    }
    
    // 0
}
