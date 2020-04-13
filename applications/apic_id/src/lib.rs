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
extern crate raw_cpuid;


use alloc::{
    vec::Vec,
    string::String,
    boxed::Box,
};
use core::sync::atomic::{AtomicUsize, Ordering};
use getopts::{Matches, Options};
use spin::Once;
use apic::get_my_apic_id;
use task::TaskRef;
use hpet::get_hpet;
use libtest::hpet_2_ns;
use heap::global_allocator;
use alloc::alloc::{GlobalAlloc, Layout};
use raw_cpuid::CpuId;


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

    match rmain() {
        Ok(_) => 0,
        Err(e) => {
            println!("Error: {}", e);
            -1
        }    
    }    
}

fn rmain() -> Result<(), &'static str> {
    let iterations = 10_000_000;

    let start = get_hpet().as_ref().ok_or("couldn't get HPET timer")?.get_counter();

    for i in 0..iterations {
        let apic = get_my_apic_id().unwrap();
    }

    let end = get_hpet().as_ref().ok_or("couldn't get HPET timer")?.get_counter();

    println!("get_my_apic_id took {} ns", hpet_2_ns(end - start) / iterations);



    let start = get_hpet().as_ref().ok_or("couldn't get HPET timer")?.get_counter();

    for i in 0..iterations {
        let apic = CpuId::new().get_feature_info().expect("Could not retrieve cpuid").initial_local_apic_id();
    }

    let end = get_hpet().as_ref().ok_or("couldn't get HPET timer")?.get_counter();

    println!("cpuid took {} ns", hpet_2_ns(end - start) / iterations);

    Ok(())
}



fn print_usage(opts: Options) {
    println!("{}", opts.usage(USAGE));
}

const USAGE: &'static str = "Usage: threadtest OPTION ARG
Provides a selection of different tests for channel-based communication.";
