#![no_std]

extern crate alloc;
#[macro_use] extern crate application_io;
#[macro_use] extern crate lazy_static;

use alloc::vec::Vec;
use alloc::string::String;
use alloc::sync::Arc;

lazy_static! {
    // Globally accessible IOProperty.
    static ref IO_PROPERTY: Arc<application_io::IOProperty> =
        application_io::claim_property().unwrap();
}

#[no_mangle]
pub fn main(_args: Vec<String>) -> isize {

    // Initialize IOProperty if it hasn't been initialized.
    lazy_static::initialize(&IO_PROPERTY);

    application_io::request_immediate_delivery(&IO_PROPERTY);
    application_io::request_no_echo(&IO_PROPERTY);

    loop {

        // Test for state spill free version
        let result = application_io::get_input_bytes(&IO_PROPERTY);
        let data = match result {
            Ok(data) => data,
            Err(_) => return 1
        };
        if let Some(line) = data {
            let parsed_line = String::from_utf8_lossy(&line);
            ssfprintln!(&IO_PROPERTY, "{}", parsed_line);
        }

        // Test for state spill free version
        let result = application_io::get_input_bytes_spilled();
        let data = match result {
            Ok(data) => data,
            Err(_) => return 1
        };
        if let Some(line) = data {
            let parsed_line = String::from_utf8_lossy(&line);
            println!("{}", parsed_line).unwrap();
        }
    }
}
