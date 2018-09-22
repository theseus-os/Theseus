//! This application is an example of how to write applications in Theseus.

#![no_std]
#![feature(alloc)]

#[macro_use] extern crate alloc;
#[macro_use] extern crate log;
#[macro_use] extern crate terminal_print;
extern crate task;
extern crate spawn;
extern crate runqueue;
extern crate getopts;
extern crate acpi;

use alloc::{Vec, String};
use getopts::{Matches, Options};
use acpi::get_hpet;


#[no_mangle]
pub fn main(args: Vec<String>) -> isize {
    let mut opts = Options::new();
    opts.optflag("h", "help", "print this help menu");
    opts.optopt("n", "", "number of iterations to run", "ITERATIONS");

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

    let result = rmain(&matches);
    match result {
        Ok(_) => { 0 }
        Err(e) => {
            println!("Runqueue evaluation failed: {}.", e);
            -1
        }
    }
}


pub fn rmain(matches: &Matches) -> Result<(), &'static str> {

    let iterations = if let Some(i) = matches.opt_str("n") {
        i.parse::<usize>().map_err(|_e| "couldn't parse number of iterations")?
    } else {
        100
    };

    #[cfg(runqueue_state_spill_evaluation)]
    let config = "WITH state spill";
    #[cfg(not(runqueue_state_spill_evaluation))]
    let config = "WITHOUT state spill";

    println!("Evaluating runqueue {} for {} iterations...", config, iterations);
    let start = get_hpet().as_ref().ok_or("couldn't get HPET timer")?.get_counter();
    
    run(iterations)?;

    let end = get_hpet().as_ref().ok_or("couldn't get HPET timer")?.get_counter();
    let hpet_period = get_hpet().as_ref().ok_or("couldn't get HPET timer")?.counter_period_femtoseconds();

    println!("Runqueue evaluation completed successfully.");
    println!("HPET Period: {} femtoseconds.", hpet_period);
    println!("Elapsed HPET ticks: {}", end - start);

    Ok(())
}


fn run(iterations: usize) -> Result<(), &'static str> {
    for i in 0..iterations {
        let taskref = spawn::KernelTaskBuilder::new(target_fn, i)
            .name(format!("rq_eval_test_{}", i))
            .spawn()?;
        taskref.join()?;
    }
    Ok(())
}


fn target_fn(_iteration: usize) -> usize {
    warn!("in target function, iteration {}.", _iteration);
    _iteration
}



fn print_usage(opts: Options) {
    println!("{}", opts.usage(USAGE));
}


const USAGE: &'static str = "Usage: rq_eval [ARGS]
Evaluates the runqueue implementation.";
