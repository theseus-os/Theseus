//! Tests the basic features of Theseus's port of `std::fs` modules from Rust `std`.
//! 
#![no_std]

extern crate alloc;
extern crate log;
extern crate theseus_std;
extern crate core2;
#[macro_use] extern crate app_io;

use alloc::{string::String, vec::Vec};
use core2::io::{self, Write};


pub fn main(_args: Vec<String>) -> isize {
    match rmain(_args) {
        Ok(_) => {
            println!("test_std_fs complete!");
            0
        }
        Err(e) => {
            println!("test_std_fs error: {:?}", e);
            -1
        }    
    }
}


fn rmain(_args: Vec<String>) -> io::Result<()> {
    let mut out = theseus_std::fs::File::create("test.txt")?;
    out.write(b"yo what's up\nhey there!")?;
    out.write_all(b"take 2: yo what's up, hey there!")?;
    Ok(())
}
