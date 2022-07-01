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

    let _res = task::set_kill_handler(Box::new(|kill_reason| {
        println!("test_panic: caught a kill action: {}", kill_reason);
    }));

    info!("test_panic::main(): registered kill handler? {:?}.", _res);

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

