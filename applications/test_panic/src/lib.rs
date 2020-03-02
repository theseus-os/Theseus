#![no_std]


extern crate alloc;
#[macro_use] extern crate log;
#[macro_use] extern crate terminal_print;
extern crate task;


use alloc::vec::Vec;
use alloc::string::String;
use alloc::boxed::Box;


pub fn main(_args: Vec<String>) -> isize {
    info!("test_panic::main(): at top");

    let _res = task::set_my_panic_handler(Box::new(|info| {
        println!("Caught a panic at {}", info);
    }));

    info!("test_panic::main(): registering panic handler... {:?}.", _res);

    match _args.get(0).map(|s| &**s) {
        // indexing test
        Some("-i") => {
            info!("test_panic::main(): trying out of bounds access...");
            warn!("test_panic unexpectedly successed: args[100] = {}", _args[100]); // this should panic
        }
        // direct panic by default
        _ => {
            info!("test_panic::main(): directly calling panic...");
            panic!("yo i'm testing a panic!!");
        }
    }

    warn!("test_panic returned successfully...? Isn't it supposed to panic?");
    0
}


// use task::PanicInfoOwned;
// fn panic_handler(info: &PanicInfoOwned) {
//     println!("Caught a panic: {}", info);
// }
