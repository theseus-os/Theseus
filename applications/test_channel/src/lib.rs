#![no_std]

extern crate alloc;
#[macro_use] extern crate log;
#[macro_use] extern crate app_io;
extern crate getopts;
extern crate spin;
extern crate task;
extern crate spawn;
extern crate scheduler;
extern crate rendezvous;
extern crate async_channel;
extern crate cpu;


use alloc::{
    vec::Vec,
    string::String,
};
use core::sync::atomic::{AtomicUsize, Ordering};
use getopts::{Matches, Options};
use spin::Once;


static VERBOSE: Once<bool> = Once::new();
#[allow(unused)]
macro_rules! verbose {
    () => (VERBOSE.get() == Some(&true));
}

static PINNED: Once<bool> = Once::new();
macro_rules! pin_task {
    ($tb:ident, $cpu:expr) => (
        if PINNED.get() == Some(&true) {
            $tb.pin_on_core($cpu)
        } else {
            $tb
        });
}

static ITERATIONS: AtomicUsize = AtomicUsize::new(100);
static SEND_COUNT: AtomicUsize = AtomicUsize::new(100);
static RECEIVE_COUNT: AtomicUsize = AtomicUsize::new(100);

macro_rules! iterations {
    () => (ITERATIONS.load(Ordering::SeqCst))
}

macro_rules! send_count {
    () => (SEND_COUNT.load(Ordering::SeqCst))
}

macro_rules! receive_count {
    () => (RECEIVE_COUNT.load(Ordering::SeqCst))
}

pub fn main(args: Vec<String>) -> isize {
    let mut opts = Options::new();
    opts.optflag("h", "help", "print this help menu");
    opts.optflag("v", "verbose", "enable verbose output");
    opts.optflag("p", "pinned", "force all test tasks to be spawned on and pinned to a single CPU. Otherwise, default behavior is spawning tasks across multiple CPUs.");
    opts.optopt("n", "iterations", "number of test iterations (default 100)", "ITER");
    opts.optopt("s", "send_count", "number of messages expected to be sent in 'multiple' test (default 100)", "SEND_C");
    opts.optopt("l", "receive_count", "number of messages expected to be received in 'multiple' test (default 100)", "REC_C");
    opts.optopt("x", "panic_in_send", "Injects a panic at specified message in sender in multiple tests (default no panic)", "SEND_PANIC");
    opts.optopt("y", "panic_in_receive", "Injects a panic at specified message in receiver in multiple tests (default no panic)", "RECEIVE_PANIC");

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

    if let Some(send_count) = matches.opt_str("s").and_then(|i| i.parse::<usize>().ok()) {
        SEND_COUNT.store(send_count, Ordering::SeqCst);
    }

    if let Some(recv_count) = matches.opt_str("l").and_then(|i| i.parse::<usize>().ok()) {
        RECEIVE_COUNT.store(recv_count, Ordering::SeqCst);
    }


    match rmain(matches) {
        Ok(_) => 0,
        Err(e) => {
            println!("Error: {}", e);
            -1
        }    
    }    
}

fn rmain(matches: Matches) -> Result<(), &'static str> {
    let mut did_something = false;

    // If the user has specified panic instances as 'val', 'send_panic_pont' will be 'Some(val)'.
    // Similarly for 'receive_panic_point' as well.
    let send_panic_point = matches.opt_str("x").and_then(|i| i.parse::<usize>().ok());
    let receive_panic_point = matches.opt_str("y").and_then(|i| i.parse::<usize>().ok());

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
            rendezvous_test_multiple(send_count!(), receive_count!(), send_panic_point, receive_panic_point)?;
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
            asynchronous_test_multiple(send_count!(), receive_count!(), send_panic_point, receive_panic_point)?;
        }
    }

    if did_something {
        println!("Test complete.");
        Ok(())
    } else {
        Err("no action performed, please select a test")
    }
}


/// A simple test that spawns a sender & receiver task to send a single message
fn rendezvous_test_oneshot() -> Result<(), &'static str> {
    let my_cpu = cpu::current_cpu();

    let (sender, receiver) = rendezvous::new_channel();

    let t1 = spawn::new_task_builder(|_: ()| -> Result<(), &'static str> {
        warn!("rendezvous_test_oneshot(): Entered sender task!");
        sender.send("hello")?;
        Ok(())
    }, ())
        .name(String::from("sender_task_rendezvous_oneshot"))
        .block();
    let t1 = pin_task!(t1, my_cpu).spawn()?;

    let t2 = spawn::new_task_builder(|_: ()| -> Result<(), &'static str> {
        warn!("rendezvous_test_oneshot(): Entered receiver task!");
        let msg = receiver.receive()?;
        warn!("rendezvous_test_oneshot(): Receiver got msg: {:?}", msg);
        Ok(())
    }, ())
        .name(String::from("receiver_task_rendezvous_oneshot"))
        .block();
    let t2 = pin_task!(t2, my_cpu).spawn()?;

    warn!("rendezvous_test_oneshot(): Finished spawning the sender and receiver tasks");
    t2.unblock().unwrap();
    t1.unblock().unwrap();

    t1.join()?;
    t2.join()?;
    warn!("rendezvous_test_oneshot(): Joined the sender and receiver tasks.");
    
    Ok(())
}



/// A simple test that spawns a sender & receiver task to send `send_count` and receive `receive_count` messages.
/// Optionally can set panics at `send_panic` and `receive_panic` locations
fn rendezvous_test_multiple(send_count: usize, receive_count: usize, send_panic: Option<usize>, receive_panic: Option<usize>) -> Result<(), &'static str> {
    let my_cpu = cpu::current_cpu();

    let (sender, receiver) = rendezvous::new_channel();

    let t1 = spawn::new_task_builder(rendezvous_sender_task, (sender, send_count, send_panic))
        .name(String::from("sender_task_rendezvous"))
        .block();
    
    let t1 = pin_task!(t1, my_cpu).spawn()?;

    let t2 = spawn::new_task_builder(rendezvous_receiver_task, (receiver, receive_count, receive_panic))
        .name(String::from("receiver_task_rendezvous"))
        .block();

    let t2 = pin_task!(t2, my_cpu).spawn()?;

    warn!("rendezvous_test_multiple(): Finished spawning the sender and receiver tasks");
    t2.unblock().unwrap();
    t1.unblock().unwrap();

    t1.join()?;
    t2.join()?;
    warn!("rendezvous_test_multiple(): Joined the sender and receiver tasks.");
    
    Ok(())
}

/// A simple receiver receiving `iterations` messages
/// Optionally may panic after sending `panic_pont` messages
fn rendezvous_receiver_task ((receiver, iterations, panic_point): (rendezvous::Receiver<String>, usize, Option<usize>)) -> Result<(), &'static str> {
    warn!("rendezvous_test(): Entered receiver task! Expecting to receive {} messages", iterations);

    if panic_point.is_some(){
        warn!("rendezvous_test(): Panic will occur in receiver task at message {}",panic_point.unwrap());
    }

    for i in 0..iterations {
        let msg = receiver.receive().map_err(|error| {
            warn!("Receiver task returned error : {}", error);
            return error;
        })?;
        warn!("rendezvous_test(): Receiver got {:?}  ({:03})", msg, i);

        if panic_point == Some(i) {
            panic!("rendezvous_test() : User specified panic in receiver");
        }
    }

    warn!("rendezvous_test(): Done receiver task!");
    Ok(())
}

/// A simple sender sending `iterations` messages
/// Optionally may panic after sending `panic_pont` messages
fn rendezvous_sender_task ((sender, iterations, panic_point): (rendezvous::Sender<String>, usize, Option<usize>)) -> Result<(), &'static str> {
    warn!("rendezvous_test(): Entered sender task! Expecting to send {} messages", iterations);

    if panic_point.is_some(){
        warn!("rendezvous_test(): Panic will occur in sender task at message {}",panic_point.unwrap());
    }

    for i in 0..iterations {
        sender.send(format!("Message {:03}", i)).map_err(|error| {
            warn!("Sender task returned error : {}", error);
            return error;
        })?;
        warn!("rendezvous_test(): Sender sent message {:03}", i);

        if panic_point == Some(i) {
            panic!("rendezvous_test() : User specified panic in receiver");
        }
    }

    warn!("rendezvous_test(): Done sender task!");
    Ok(())
}


/// A simple test that spawns a sender & receiver task to send a single message
/// Optionally can set panics at `send_panic` and `receive_panic` locations
fn asynchronous_test_oneshot() -> Result<(), &'static str> {
    let my_cpu = cpu::current_cpu();

    let (sender, receiver) = async_channel::new_channel(2);

    let t1 = spawn::new_task_builder(|_: ()| -> Result<(), &'static str> {
        warn!("asynchronous_test_oneshot(): Entered sender task!");
        sender.send("hello").map_err(|error| {
            warn!("Sender task failed due to : {:?}", error);
            return "Sender task failed";
        })?;
        Ok(())
    }, ())
        .name(String::from("sender_task_asynchronous_oneshot"))
        .block();
    let t1 = pin_task!(t1, my_cpu).spawn()?;

    let t2 = spawn::new_task_builder(|_: ()| -> Result<(), &'static str> {
        warn!("asynchronous_test_oneshot(): Entered receiver task!");
        let msg = receiver.receive().map_err(|error| {
            warn!("Receiver task failed due to : {:?}", error);
            return "Receiver task failed"
        })?;
        warn!("asynchronous_test_oneshot(): Receiver got msg: {:?}", msg);
        Ok(())
    }, ())
        .name(String::from("receiver_task_asynchronous_oneshot"))
        .block();
    let t2 = pin_task!(t2, my_cpu).spawn()?;

    warn!("asynchronous_test_oneshot(): Finished spawning the sender and receiver tasks");
    t2.unblock().unwrap();
    t1.unblock().unwrap();

    t1.join()?;
    t2.join()?;
    warn!("asynchronous_test_oneshot(): Joined the sender and receiver tasks.");
    
    Ok(())
}


/// A simple test that spawns a sender & receiver task to send `send_count` and receive `receive_count` messages.
fn asynchronous_test_multiple(send_count: usize, receive_count: usize, send_panic: Option<usize>, receive_panic: Option<usize>) -> Result<(), &'static str> {
    let my_cpu = cpu::current_cpu();

    let (sender, receiver) = async_channel::new_channel(2);

    let t1 = spawn::new_task_builder(asynchronous_sender_task, (sender, send_count, send_panic))
        .name(String::from("sender_task_asynchronous"))
        .block();

    let t1 = pin_task!(t1, my_cpu).spawn()?;

    let t2 = spawn::new_task_builder(asynchronous_receiver_task, (receiver, receive_count, receive_panic))
        .name(String::from("receiver_task_asynchronous"))
        .block();

    let t2 = pin_task!(t2, my_cpu).spawn()?;

    warn!("asynchronous_test_multiple(): Finished spawning the sender and receiver tasks");
    t2.unblock().unwrap();
    t1.unblock().unwrap();

    t1.join()?;
    t2.join()?;
    warn!("asynchronous_test_multiple(): Joined the sender and receiver tasks.");
    
    Ok(())
}

/// A simple receiver receiving `iterations` messages
/// Optionally may panic after sending `panic_pont` messages
fn asynchronous_receiver_task ((receiver, iterations, panic_point): (async_channel::Receiver<String>, usize, Option<usize>)) -> Result<(), &'static str> {
    warn!("asynchronous_test(): Entered receiver task! Expecting to receive {} messages", iterations);
    
    if panic_point.is_some(){
        warn!("asynchronous_test(): Panic will occur in receiver task at message {}",panic_point.unwrap());
    }

    for i in 0..iterations {
        let msg = receiver.receive().map_err(|error| {
            warn!("Receiver task failed due to : {:?}", error);
            return "Receiver task failed"
        })?;
        warn!("asynchronous_test(): Receiver got {:?}  ({:03})", msg, i);

        if panic_point == Some(i) {
            panic!("rendezvous_test() : User specified panic in receiver");
        }
    }

    warn!("asynchronous_test(): Done receiver task!");
    Ok(())
}

/// A simple sender sending `iterations` messages
/// Optionally may panic after sending `panic_pont` messages
fn asynchronous_sender_task ((sender, iterations, panic_point): (async_channel::Sender<String>, usize, Option<usize>)) -> Result<(), &'static str> {
    warn!("asynchronous_test(): Entered sender task! Expecting to send {} messages", iterations);
    if panic_point.is_some(){
        warn!("asynchronous_test(): Panic will occur in sender task at message {}",panic_point.unwrap());
    }

    for i in 0..iterations {
        sender.send(format!("Message {:03}", i)).map_err(|error| {
            warn!("Sender task failed due to : {:?}", error);
            return "Sender task failed";
        })?;
        warn!("asynchronous_test(): Sender sent message {:03}", i);

        if panic_point == Some(i) {
            panic!("rendezvous_test() : User specified panic in receiver");
        }
    }

    warn!("asynchronous_test(): Done sender task!");
    Ok(())
}


fn print_usage(opts: Options) {
    println!("{}", opts.usage(USAGE));
}

const USAGE: &'static str = "Usage: test_channel OPTION ARG
Provides a selection of different tests for channel-based communication.";
