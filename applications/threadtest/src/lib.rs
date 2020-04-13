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
extern crate hpet;
#[macro_use] extern crate libtest;
extern crate heap;


use alloc::{
    vec::Vec,
    string::String,
    boxed::Box,
};
use core::sync::atomic::{AtomicUsize, Ordering};
use getopts::{Matches, Options};
use spin::Once;
use apic::get_lapics;
use task::TaskRef;
use hpet::get_hpet;
use libtest::hpet_2_ns;
use heap::global_allocator;
use alloc::alloc::{GlobalAlloc, Layout};


static VERBOSE: Once<bool> = Once::new();
#[allow(unused)]
macro_rules! verbose {
    () => (VERBOSE.try() == Some(&true));
}


static NTHREADS: AtomicUsize = AtomicUsize::new(1);
macro_rules! threads {
    () => (THREADS.load(Ordering::SeqCst))
}

const NITERATIONS: usize = 50;
const NOBJECTS: usize = 30_000;
const WORK: usize = 0;
const OBJSIZE: usize = 1;


pub fn main(args: Vec<String>) -> isize {
    let mut opts = Options::new();
    opts.optflag("h", "help", "print this help menu");
    opts.optflag("v", "verbose", "enable verbose output");
    opts.optopt("t", "threads", "number of threads to spawnon separate cores", "THREADS");
    
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
    if let Some(threads) = matches.opt_str("t").and_then(|i| i.parse::<usize>().ok()) {
        NTHREADS.store(threads, Ordering::SeqCst);
    }

    match rmain() {
        Ok(_) => 0,
        Err(e) => {
            println!("Error: {}", e);
            -1
        }    
    }    
}

fn rmain() -> Result<(), &'static str> {

    let nthreads = NTHREADS.load(Ordering::SeqCst);

    println!("Running threadtest for {} threads, {} iterations, {} work and {} obj size ...", nthreads, NITERATIONS, WORK, OBJSIZE);

    let mut threads = Vec::new();

    let start = get_hpet().as_ref().ok_or("couldn't get HPET timer")?.get_counter();

    for i in 0..nthreads {
        threads.push(spawn::new_task_builder(worker, ()).name(String::from("worker thread")).spawn()?);
    }  

    for i in 0..nthreads {
        threads[i].join()?;
    }

    let end = get_hpet().as_ref().ok_or("couldn't get HPET timer")?.get_counter();

    println!("threadtest took {} ns", hpet_2_ns(end - start));

    Ok(())
}

struct Foo {
    x: i32,
    y: i32
}

impl Foo {
    const fn empty() -> Foo {
        Foo{ x: 0, y: 0}
    }

    fn new() -> Foo {
        Foo{ x: 14, y: 29 }
    }
}

fn worker(_:()) {
    // error!("Worker thread running on core: {}", CPU_ID!());

    let niterations = NITERATIONS;
    const nobjects: usize = NOBJECTS;
    let nthreads: usize = NTHREADS.load(Ordering::SeqCst);

    let layout = Layout::from_size_align(8, 8).unwrap();
    let heap = global_allocator();

    for j in 0..niterations {
        let mut a = Vec::with_capacity(nobjects/nthreads);
        unsafe{
        for i in 0..(nobjects/nthreads) {
            let obj = heap.alloc(layout); 
            a.push(obj);
        }

        for i in 0..(nobjects/nthreads) {
            let obj = a[i];
            heap.dealloc(obj, layout);
        }
        }
    }
}




fn print_usage(opts: Options) {
    println!("{}", opts.usage(USAGE));
}

const USAGE: &'static str = "Usage: threadtest OPTION ARG
Provides a selection of different tests for channel-based communication.";
