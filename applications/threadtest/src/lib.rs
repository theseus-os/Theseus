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
extern crate heap_trace;
extern crate pmu_x86;

use alloc::{
    vec::Vec,
    string::String,
    boxed::Box,
};
use core::sync::atomic::{AtomicUsize, AtomicBool, Ordering};
use getopts::{Matches, Options};
use spin::Once;
use apic::get_lapics;
use task::TaskRef;
use hpet::get_hpet;
use libtest::hpet_2_ns;
use heap::global_allocator;
use alloc::alloc::{GlobalAlloc, Layout};
use heap_trace::{start_heap_trace, stop_heap_trace, calclulate_time_per_step, NSTEPS};
use pmu_x86::EventType;

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

static START_ITER: AtomicBool = AtomicBool::new(false);


pub fn main(args: Vec<String>) -> isize {
    let _ = pmu_x86::init();
    println!("hello {}", libtest::cycle_count_overhead().unwrap());

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


    for i in 0..nthreads {
        threads.push(spawn::new_task_builder(worker, ()).name(String::from("worker thread")).spawn()?);
    }  

    let start = get_hpet().as_ref().ok_or("couldn't get HPET timer")?.get_counter();

    // START_ITER.store(true, Ordering::SeqCst);


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
    let mut a = Vec::with_capacity(nobjects/nthreads);
    unsafe { a.set_len(nobjects/nthreads); }
    let heap = global_allocator();

    // let mut avg_times: [u64;NSTEPS] = [0; NSTEPS];

    for j in 0..niterations {
        // start_heap_trace();
        
        unsafe{
        for i in 0..(nobjects/nthreads) {
            let obj = heap.alloc(layout); 
            a[i] = obj;
        }

        for i in 0..(nobjects/nthreads) {
            let obj = a[i];
            heap.dealloc(obj, layout);
        }
        }
        // stop_heap_trace();
        // // heap_trace::print_heap_trace_from_index(7, 14);
        // let times = calclulate_time_per_step();
        // for i in 0..NSTEPS {
        //     avg_times[i] += times[i];
        // }
    }

    // for time in &mut avg_times {
    //     *time /= (niterations as u64);
    // }

    // error!("Time spend in each step: {:?}", avg_times);
}




fn print_usage(opts: Options) {
    println!("{}", opts.usage(USAGE));
}

const USAGE: &'static str = "Usage: threadtest OPTION ARG
Provides a selection of different tests for channel-based communication.";
