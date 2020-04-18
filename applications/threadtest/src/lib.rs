//! A Rust version of the threadtest heap microbenchmark
//! 
//! The original version was presented in the Hoard paper
//! https://github.com/emeryberger/Hoard/tree/master/benchmarks/threadtest

#![no_std]

extern crate alloc;
#[macro_use] extern crate terminal_print;
extern crate getopts;
extern crate spawn;
extern crate hpet;
extern crate libtest;

use alloc::{
    vec::Vec,
    string::String,
    boxed::Box,
};
use core::sync::atomic::{AtomicUsize, Ordering};
use getopts::Options;
use hpet::get_hpet;
use libtest::hpet_2_ns;


static NTHREADS: AtomicUsize = AtomicUsize::new(1);
const NITERATIONS: usize = 50;
/// Sum total of objects to be allocated by all threads
const NOBJECTS: usize = 30_000;
/// Size of the objects we're allocating in bytes
const OBJSIZE: usize = 8;


pub fn main(args: Vec<String>) -> isize {
    let mut opts = Options::new();
    opts.optflag("h", "help", "print this help menu");
    opts.optopt("t", "threads", "number of worker threads to spawn on separate cores", "THREADS");
    
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
    let mut threads = Vec::with_capacity(nthreads);
    let hpet = get_hpet(); 
    println!("Running threadtest for {} threads, {} iterations, {} obj size ...", nthreads, NITERATIONS, OBJSIZE);

    let start = hpet.as_ref().ok_or("couldn't get HPET timer")?.get_counter();

    for _ in 0..nthreads {
        threads.push(spawn::new_task_builder(worker, ()).name(String::from("worker thread")).spawn()?);
    }  

    for i in 0..nthreads {
        threads[i].join()?;
        threads[i].take_exit_value();
    }

    let end = hpet.as_ref().ok_or("couldn't get HPET timer")?.get_counter();
    println!("threadtest took {} ns", hpet_2_ns(end - start));

    Ok(())
}


struct Foo {
    pub x: i32,
    pub y: i32
}

impl Foo {
    fn new() -> Foo {
        Foo{ x: 14, y: 29 }
    }
}


fn worker(_:()) {
    let niterations = NITERATIONS;
    let nobjects = NOBJECTS;
    let nthreads = NTHREADS.load(Ordering::SeqCst);

    for _ in 0..niterations {
        let mut a = Vec::with_capacity(nobjects/nthreads);
        for _ in 0..(nobjects/nthreads) {
            let obj = Box::new(Foo::new()); 
            a.push(obj)
        }

        for obj in a {
            core::mem::drop(obj);
        }
    }
}


fn print_usage(opts: Options) {
    println!("{}", opts.usage(USAGE));
}

const USAGE: &'static str = "Usage: threadtest OPTION ARG";
