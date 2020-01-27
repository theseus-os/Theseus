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
use spawn::KernelTaskBuilder;


static ITERATIONS: AtomicUsize = AtomicUsize::new(10);
macro_rules! iterations {
    () => (ITERATIONS.load(Ordering::SeqCst))
}


#[no_mangle]
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
            println!("channel_app completed successfully.");
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
    let my_cpu = apic::get_my_apic_id().ok_or("couldn't get my APIC ID")?;

    let (sender, receiver) = unified_channel::new_channel(2);

    let t1 = KernelTaskBuilder::new(|_: ()| -> Result<(), &'static str> {
        warn!("test_multiple(): Entered sender task!");
        for i in 0..iterations {
            sender.send(format!("Message {:03}", i))?;
            warn!("test_multiple(): Sender sent message {:03}", i);
        }
        Ok(())
    }, ())
        .name(String::from("sender_task"))
        .block()
        .pin_on_core(my_cpu)
        .spawn()?;

    let t2 = KernelTaskBuilder::new(|_: ()| -> Result<(), &'static str> {
        warn!("test_multiple(): Entered receiver task!");
        for i in 0..iterations {
            let msg = receiver.receive()?;
            warn!("test_multiple(): Receiver got {:?}  ({:03})", msg, i);
        }
        Ok(())
    }, ())
        .name(String::from("receiver_task"))
        .block()
        .pin_on_core(my_cpu)
        .spawn()?;

    warn!("test_multiple(): Finished spawning the sender and receiver tasks");
    t2.unblock(); t1.unblock();

    t1.join()?;
    t2.join()?;
    warn!("test_multiple(): Joined the sender and receiver tasks.");
    
    Ok(())
}


fn print_usage(opts: Options) {
    println!("{}", opts.usage(USAGE));
}


const USAGE: &'static str = "Usage: channel_app [ARGS]
Used for evaluating live evolution between sync/async channels in Theseus.";
