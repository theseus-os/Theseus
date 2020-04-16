#![no_std]

extern crate alloc;
#[macro_use] extern crate terminal_print;
extern crate getopts;
extern crate spin;
extern crate task;
extern crate spawn;
extern crate scheduler;
extern crate rendezvous;
extern crate async_channel;
extern crate hpet;
extern crate libtest;
extern crate heap;

use alloc::{
    vec::Vec,
    string::String,
};
use core::sync::atomic::{AtomicUsize, Ordering};
use getopts::Options;
use hpet::get_hpet;
use libtest::hpet_2_ns;
use heap::global_allocator;
use alloc::alloc::{GlobalAlloc, Layout};


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

    println!("Running threadtest for {} threads, {} iterations, {} obj size ...", nthreads, NITERATIONS, OBJSIZE);

    let mut threads = Vec::new();
    let start = get_hpet().as_ref().ok_or("couldn't get HPET timer")?.get_counter();

    for _ in 0..nthreads {
        threads.push(spawn::new_task_builder(worker, ()).name(String::from("worker thread")).spawn()?);
    }  

    for i in 0..nthreads {
        threads[i].join()?;
    }

    let end = get_hpet().as_ref().ok_or("couldn't get HPET timer")?.get_counter();
    println!("threadtest took {} ns", hpet_2_ns(end - start));

    Ok(())
}


fn worker(_:()) {
    let niterations = NITERATIONS;
    let nobjects = NOBJECTS;
    let nthreads = NTHREADS.load(Ordering::SeqCst);

    let layout = Layout::from_size_align(OBJSIZE, OBJSIZE).unwrap();
    let mut a = Vec::with_capacity(nobjects/nthreads);
    // This is safe since we created with this capacity. 
    // We set the length so that there's no additional allocation within the routine.
    unsafe { a.set_len(nobjects/nthreads); } 
    let heap = global_allocator();

    for _ in 0..niterations {
        for i in 0..(nobjects/nthreads) {
            let obj = unsafe{ heap.alloc(layout) }; 
            a[i] = obj;
        }

        for i in 0..(nobjects/nthreads) {
            let obj = a[i];
            unsafe{ heap.dealloc(obj, layout) };
        }
    }
}


fn print_usage(opts: Options) {
    println!("{}", opts.usage(USAGE));
}

const USAGE: &'static str = "Usage: threadtest OPTION ARG";
