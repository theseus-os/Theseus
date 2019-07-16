#![no_std]


extern crate alloc;
extern crate application_io;
#[macro_use] extern crate terminal_print;

use alloc::vec::Vec;
use alloc::string::String;


#[no_mangle]
pub fn main(_args: Vec<String>) -> isize {
    match application_io::request_kbd_event_forward() {
        Ok(_) => {},
        Err(_) => return 1
    };
    loop {

        let result = application_io::get_keyboard_event();

        let event = match result {
            Ok(event) => event,
            Err(_) => return 1
        };

        match event {
            Some(_event) => println!("event received"),
            _ => {}
        };
    }
}
