#![no_std]

#[macro_use] extern crate alloc;
#[macro_use] extern crate log;
#[macro_use] extern crate terminal_print;
extern crate getopts;
extern crate spin;
extern crate task;
extern crate spawn;
extern crate scheduler;
extern crate rendezvous;
extern crate async_channel;
extern crate apic;


use alloc::{
    vec::Vec,
    string::String,
};
use core::sync::atomic::{AtomicUsize, Ordering};
use getopts::{Matches, Options};
use spawn::KernelTaskBuilder;
use spin::Once;


static VERBOSE: Once<bool> = Once::new();
#[allow(unused)]
macro_rules! verbose {
    () => (VERBOSE.try() == Some(&true));
}

static PINNED: Once<bool> = Once::new();
macro_rules! pin_task {
    ($tb:ident, $cpu:expr) => (
        if PINNED.try() == Some(&true) {
            $tb.pin_on_core($cpu)
        } else {
            $tb
        });
}

static ITERATIONS: AtomicUsize = AtomicUsize::new(100);
macro_rules! iterations {
    () => (ITERATIONS.load(Ordering::SeqCst))
}


#[no_mangle]
pub fn main(args: Vec<String>) -> isize {
    let mut opts = Options::new();
    opts.optflag("h", "help", "print this help menu");
    opts.optflag("v", "verbose", "enable verbose output");
    opts.optflag("p", "pinned", "force all test tasks to be spawned on and pinned to a single CPU. Otherwise, default behavior is spawning tasks across multiple CPUs.)");
    opts.optopt("n", "iterations", "number of test iterations (default 100)", "ITER");

    opts.optflag("r", "rendezvous", "run the test on the rendezvous-based synchronous channel");
    opts.optflag("a", "asynchronous", "run the test on the asynchronous buffered channel");
    opts.optflag("o", "oneshot", "run the 'oneshot' test variant, in which {ITER} tasks are spawned to send/receive one message each.");
    opts.optflag("m", "multiple", "run the 'multiple' test, in which one sender and one receiver task are spawned to send/receive {ITER} messages.");
    
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

    VERBOSE.call_once(|| matches.opt_present("v"));
    PINNED.call_once(|| matches.opt_present("p"));
    if let Some(iters) = matches.opt_str("n").and_then(|i| i.parse::<usize>().ok()) {
        ITERATIONS.store(iters, Ordering::SeqCst);
    }

    match rmain(matches) {
        Ok(_) => 0,
        Err(e) => {
            println!("Error:\n{}", e);
            -1
        }    
    }    
}

fn rmain(matches: Matches) -> Result<(), &'static str> {
    let mut did_something = false; 
    if matches.opt_present("r") {
        if matches.opt_present("o") {
            did_something = true;
            println!("Running rendezvous channel test in oneshot mode.");
            for _i in 0 .. iterations!() {
                rendezvous_test_oneshot()?;
            }
        }
        if matches.opt_present("m") {
            did_something = true;
            println!("Running rendezvous channel test in multiple mode.");
            rendezvous_test_multiple(iterations!())?;
        }
    }

    if matches.opt_present("a") {
        if matches.opt_present("o") {
            did_something = true;
            println!("Running asynchronous channel test in oneshot mode.");
            for _i in 0 .. iterations!() {
                asynchronous_test_oneshot()?;
            }
        }
        if matches.opt_present("m") {
            did_something = true;
            println!("Running asynchronous channel test in multiple mode.");
            asynchronous_test_multiple(iterations!())?;
        }
    }

    if did_something {
        Ok(())
    } else {
        Err("no action performed, please select a test")
    }
}


/// A simple test that spawns a sender & receiver task to send a single message
fn rendezvous_test_oneshot() -> Result<(), &'static str> {
    let my_cpu = apic::get_my_apic_id().ok_or("couldn't get my APIC ID")?;

    let (sender, receiver) = rendezvous::new_channel();

    let t1 = KernelTaskBuilder::new(|_: ()| -> Result<(), &'static str> {
        warn!("rendezvous_test_oneshot(): Entered sender task!");
        sender.send("hello")?;
        Ok(())
    }, ())
        .name(String::from("sender_task_rendezvous_oneshot"))
        .block();
    let t1 = pin_task!(t1, my_cpu).spawn()?;

    let t2 = KernelTaskBuilder::new(|_: ()| -> Result<(), &'static str> {
        warn!("rendezvous_test_oneshot(): Entered receiver task!");
        let msg = receiver.receive()?;
        warn!("rendezvous_test_oneshot(): Receiver got msg: {:?}", msg);
        Ok(())
    }, ())
        .name(String::from("receiver_task_rendezvous_oneshot"))
        .block();
    let t2 = pin_task!(t2, my_cpu).spawn()?;

    warn!("rendezvous_test_oneshot(): Finished spawning the sender and receiver tasks");
    t2.unblock(); t1.unblock();

    t1.join()?;
    t2.join()?;
    warn!("rendezvous_test_oneshot(): Joined the sender and receiver tasks.");
    
    Ok(())
}



/// A simple test that spawns a sender & receiver task to send `iterations` messages.
fn rendezvous_test_multiple(iterations: usize) -> Result<(), &'static str> {
    let my_cpu = apic::get_my_apic_id().ok_or("couldn't get my APIC ID")?;

    let (sender, receiver) = rendezvous::new_channel();

    let t1 = KernelTaskBuilder::new(|_: ()| -> Result<(), &'static str> {
        warn!("rendezvous_test_multiple(): Entered sender task!");
        for i in 0..iterations {
            sender.send(format!("Message {:03}", i))?;
            warn!("rendezvous_test_multiple(): Sender sent message {:03}", i);
        }
        Ok(())
    }, ())
        .name(String::from("sender_task"))
        .block();
    let t1 = pin_task!(t1, my_cpu).spawn()?;

    let t2 = KernelTaskBuilder::new(|_: ()| -> Result<(), &'static str> {
        warn!("rendezvous_test_multiple(): Entered receiver task!");
        for i in 0..iterations {
            let msg = receiver.receive()?;
            warn!("rendezvous_test_multiple(): Receiver got {:?}  ({:03})", msg, i);
        }
        Ok(())
    }, ())
        .name(String::from("receiver_task"))
        .block();
    let t2 = pin_task!(t2, my_cpu).spawn()?;

    warn!("rendezvous_test_multiple(): Finished spawning the sender and receiver tasks");
    t2.unblock(); t1.unblock();

    t1.join()?;
    t2.join()?;
    warn!("rendezvous_test_multiple(): Joined the sender and receiver tasks.");
    
    Ok(())
}


/// A simple test that spawns a sender & receiver task to send a single message
fn asynchronous_test_oneshot() -> Result<(), &'static str> {
    let my_cpu = apic::get_my_apic_id().ok_or("couldn't get my APIC ID")?;

    let (sender, receiver) = async_channel::new_channel(2);

    let t1 = KernelTaskBuilder::new(|_: ()| -> Result<(), &'static str> {
        warn!("asynchronous_test_oneshot(): Entered sender task!");
        sender.send("hello")?;
        Ok(())
    }, ())
        .name(String::from("sender_task_asynchronous_oneshot"))
        .block();
    let t1 = pin_task!(t1, my_cpu).spawn()?;

    let t2 = KernelTaskBuilder::new(|_: ()| -> Result<(), &'static str> {
        warn!("asynchronous_test_oneshot(): Entered receiver task!");
        let msg = receiver.receive()?;
        warn!("asynchronous_test_oneshot(): Receiver got msg: {:?}", msg);
        Ok(())
    }, ())
        .name(String::from("receiver_task_asynchronous_oneshot"))
        .block();
    let t2 = pin_task!(t2, my_cpu).spawn()?;

    warn!("asynchronous_test_oneshot(): Finished spawning the sender and receiver tasks");
    t2.unblock(); t1.unblock();

    t1.join()?;
    t2.join()?;
    warn!("asynchronous_test_oneshot(): Joined the sender and receiver tasks.");
    
    Ok(())
}



/// A simple test that spawns a sender & receiver task to send `iterations` messages.
fn asynchronous_test_multiple(iterations: usize) -> Result<(), &'static str> {
    let my_cpu = apic::get_my_apic_id().ok_or("couldn't get my APIC ID")?;

    let (sender, receiver) = async_channel::new_channel(2);

    let t1 = KernelTaskBuilder::new(|_: ()| -> Result<(), &'static str> {
        warn!("asynchronous_test_multiple(): Entered sender task!");
        for i in 0..iterations {
            sender.send(format!("Message {:03}", i))?;
            warn!("asynchronous_test_multiple(): Sender sent message {:03}", i);
        }
        Ok(())
    }, ())
        .name(String::from("sender_task_asynchronous_multiple"))
        .block();
    let t1 = pin_task!(t1, my_cpu).spawn()?;

    let t2 = KernelTaskBuilder::new(|_: ()| -> Result<(), &'static str> {
        warn!("asynchronous_test_multiple(): Entered receiver task!");
        for i in 0..iterations {
            let msg = receiver.receive()?;
            warn!("asynchronous_test_multiple(): Receiver got {:?}  ({:03})", msg, i);
        }
        Ok(())
    }, ())
        .name(String::from("receiver_task_asynchronous_multiple"))
        .block();
    let t2 = pin_task!(t2, my_cpu).spawn()?;

    warn!("asynchronous_test_multiple(): Finished spawning the sender and receiver tasks");
    t2.unblock(); t1.unblock();

    t1.join()?;
    t2.join()?;
    warn!("asynchronous_test_multiple(): Joined the sender and receiver tasks.");
    
    Ok(())
}


fn print_usage(opts: Options) {
    println!("{}", opts.usage(USAGE));
}

const USAGE: &'static str = "Usage: test_channel OPTION ARG
Provides a selection of different tests for channel-based communication.";
