#![no_std]
// #![feature(plugin)]
// #![plugin(application_main_fn)]


extern crate alloc;
// #[macro_use] extern crate log;
#[macro_use] extern crate terminal_print;
extern crate ipc;

use alloc::vec::Vec;
use alloc::string::String;



#[no_mangle]
pub fn main(_args: Vec<String>) -> isize {
    // let mut (cycles, bytes);

    let rs = ipc::test_ipc();

    match rs {
        Ok(_) => {
            println!("IPC tested successfully");
        }
        Err(err) => { 
            println!("IPC test failed");
            // return Err(err);
        }
    }
    0
}
