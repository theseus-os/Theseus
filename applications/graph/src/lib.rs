#![no_std]
#![feature(alloc)]
// #![feature(plugin)]
// #![plugin(application_main_fn)]


extern crate alloc;
#[macro_use] extern crate log;
#[macro_use] extern crate terminal_print;

extern crate frame_buffer;
extern crate acpi;
use alloc::{Vec, String};

#[no_mangle]
pub fn main(_args: Vec<String>) -> isize {
    //println!("Hello, world! Args: {:?}", _args);
    
    let mut size = 100;
    while size <= 600 {
        let mut color:u32 = 0x0000FF;
        let hpet_lock = acpi::get_hpet();
        let STARTING_TIME = unsafe { hpet_lock.as_ref().unwrap().get_counter()};
        while color != 0x00FF00 {
            frame_buffer::fill_rectangle(200, 100, size, size, color);
            color = color - 1 + 0x000100;
        }
        frame_buffer::fill_rectangle(200, 100, size, size, color);
        let hpet_lock = acpi::get_hpet();
        unsafe { 
            let END_TIME = hpet_lock.as_ref().unwrap().get_counter() - STARTING_TIME; 
            trace!("{}", END_TIME);
        }
        size += 50;
    }

    0
}
