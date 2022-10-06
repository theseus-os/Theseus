//! This application is used as a demo for evaluating Theseus's live evolution
//! between synchronous and asynchronous channels.

#![no_std]

#[macro_use] extern crate alloc;
#[macro_use] extern crate log;
#[macro_use] extern crate terminal_print;
extern crate getopts;
extern crate unified_channel;
extern crate task;
extern crate spawn;
extern crate apic;

use core::sync::atomic::{AtomicUsize, Ordering};
use alloc::vec::Vec;
use alloc::string::String;
use getopts::{Options, Matches};


static ITERATIONS: AtomicUsize = AtomicUsize::new(10);
macro_rules! iterations {
    () => (ITERATIONS.load(Ordering::SeqCst))
}


pub fn main(args: Vec<String>) -> isize {
    let mut opts = Options::new();
    opts.optflag("h", "help", "print this help menu");
    opts.optopt("n", "iterations", "number of test iterations (default 100)", "ITER");

    let matches = match opts.parse(&args) {
        Ok(m) => m,
        Err(_f) => {
            println!("{}", _f);
            print_usage(opts);
            return -1; 
        }
    };

    if matches.opt_present("h") {
        print_usage(opts);
        return 0;
    }


    match rmain(matches) {
        Ok(_) => {
            println!("channel_eval completed successfully.");
            0
        }
        Err(e) => {
            println!("Error: {}", e);
            -1
        }    
    }
}


fn rmain(_matches: Matches) -> Result<(), &'static str> {
    test_multiple(iterations!())
}



/// A simple test that spawns a sender & receiver task to send `iterations` messages.
fn test_multiple(iterations: usize) -> Result<(), &'static str> {
    let my_cpu = apic::get_my_apic_id();

    let (sender, receiver) = unified_channel::new_string_channel(2);

    let t1 = spawn::new_task_builder(|_: ()| -> Result<(), &'static str> {
        info!("test_multiple(): Entered sender task!");
        for i in 0..iterations {
            sender.send(format!("Message {:03}", i))?;
            info!("test_multiple(): Sender sent message {:03}", i);
        }
        Ok(())
    }, ())
        .name(String::from("sender_task"))
        .block()
        .pin_on_core(my_cpu)
        .spawn()?;

    let t2 = spawn::new_task_builder(|_: ()| -> Result<(), &'static str> {
        info!("test_multiple(): Entered receiver task!");
        for i in 0..iterations {
            let msg = receiver.receive()?;
            info!("test_multiple(): Receiver got {:?}  ({:03})", msg, i);
        }
        Ok(())
    }, ())
        .name(String::from("receiver_task"))
        .block()
        .pin_on_core(my_cpu)
        .spawn()?;

    info!("test_multiple(): Finished spawning the sender and receiver tasks");
    t2.unblock(); t1.unblock();

    t1.join()?;
    t2.join()?;
    info!("test_multiple(): Joined the sender and receiver tasks.");
    let _t1_exit = t1.take_exit_value();
    let _t2_exit = t2.take_exit_value();
    
    Ok(())
}


fn print_usage(opts: Options) {
    println!("{}", opts.usage(USAGE));
}


const USAGE: &'static str = "Usage: channel_eval [ARGS]
Used for evaluating live evolution between sync/async channels in Theseus.";
