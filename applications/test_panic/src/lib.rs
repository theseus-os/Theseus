#![no_std]
#![feature(alloc)]


extern crate alloc;
#[macro_use] extern crate log;
#[macro_use] extern crate terminal_print;
extern crate task;


use alloc::vec::Vec;
use alloc::string::String;
use alloc::boxed::Box;


#[no_mangle]
pub fn main(_args: Vec<String>) -> isize {
    info!("test_panic::main(): at top");

    let _res = task::set_my_panic_handler(Box::new(|info| {
        println!("Caught a panic at {}", info);
    }));

    info!("test_panic::main(): registering panic handler: {:?}. Calling panic...", _res);


    panic!("yo i'm testing a panic!!");
}


// use task::PanicInfoOwned;
// fn panic_handler(info: &PanicInfoOwned) {
//     println!("Caught a panic: {}", info);
// }
