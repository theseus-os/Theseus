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

    application_io::request_kbd_event_forward(&IO_PROPERTY);
    loop {

        // Test for state spill free version
        let result = application_io::get_keyboard_event(&IO_PROPERTY);
        let event = match result {
            Ok(event) => event,
            Err(_) => return 1
        };
        match event {
            Some(_event) => ssfprintln!(&IO_PROPERTY, "event received"),
            _ => {}
        };

        // Test for non state spill free version
        let result = application_io::get_keyboard_event_spilled();
        let event = match result {
            Ok(event) => event,
            Err(_) => return 1
        };
        match event {
            Some(_event) => println!("event received").unwrap(),
            _ => {}
        };
    }
}
