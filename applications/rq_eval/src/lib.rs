//! This application tests the performance of the runqueue implementation,
//! which is used to compare a standard runqueue with a state spill-free runqueue.
//! 
//! # Instructions for Running
//! You can run the experiments as such:
//! * `rq_eval -w 100`
//! * `rq_eval -s 100`
//! Larger iteration values should be used to eliminate the constant overhead
//! or jitter due to random context switches.
//! 
//! See the options in the main function for more details.
//! 

#![no_std]

#[macro_use] extern crate log;
extern crate alloc;
#[macro_use] extern crate app_io;
extern crate task;
extern crate cpu;
extern crate spawn;
extern crate runqueue;
extern crate getopts;
extern crate hpet;
extern crate libtest;

use alloc::{
    string::String,
    vec::Vec,
};
use getopts::{Matches, Options};
use hpet::get_hpet;
use task::{Task, TaskRef};
use libtest::{hpet_timing_overhead, hpet_2_us};


const CONFIG: &'static str = "WITHOUT state spill";

const _FEMTOSECONDS_PER_SECOND: u64 = 1000*1000*1000*1000*1000; // 10^15


// #[cfg(not(rq_eval))]
// pub fn main(args: Vec<String>) -> isize {
//     println!("Error: the \"rq_eval\" cfg option must be enabled!");
//     -1
// }

// #[cfg(rq_eval)]
pub fn main(args: Vec<String>) -> isize {

    let mut opts = Options::new();
    opts.optflag("h", "help", "print this help menu");
    opts.optopt("w", "whole", "spawn N whole empty tasks and run them each to completion", "N");
    opts.optopt("s", "single", "spawn a single task and add/remove it from various runqueues N times", "N");

    let matches = match opts.parse(&args) {
        Ok(m) => m,
        Err(_f) => {
            println!("{}", _f);
            print_usage(&opts);
            return -1; 
        }
    };

    if matches.opt_present("h") {
        print_usage(&opts);
        return 0;
    }

    let result = rmain(&matches, &opts);
    match result {
        Ok(_) => { 0 }
        Err(e) => {
            println!("Runqueue evaluation failed: {}.", e);
            -1
        }
    }
}


pub fn rmain(matches: &Matches, opts: &Options) -> Result<(), &'static str> {

    let mut did_work = false;

    if let Some(i) = matches.opt_str("w") {
        let num_tasks = i.parse::<usize>().map_err(|_e| "couldn't parse number of num_tasks")?;
        run_whole(num_tasks)?;
        did_work = true;
    }

    if let Some(i) = matches.opt_str("s") {
        let iterations = i.parse::<usize>().map_err(|_e| "couldn't parse number of num_tasks")?;
        run_single(iterations)?;
        did_work = true;
    }   

    if did_work {
        Ok(())
    }
    else {
        println!("Nothing was done. Please specify a type of evaluation task to run.");
        print_usage(opts);
        Ok(())
    }
}


fn run_whole(num_tasks: usize) -> Result<(), &'static str> {
    println!("Evaluating runqueue {} with WHOLE tasks, {} tasks...", CONFIG, num_tasks);
    
    let mut tasks = Vec::with_capacity(num_tasks);
    let overhead = hpet_timing_overhead()?;
    
    let hpet = get_hpet().ok_or("couldn't get HPET timer")?;
    let start = hpet.get_counter();

    for i in 0..num_tasks {
        let taskref = spawn::new_task_builder(whole_task, i)
            .name(format!("rq_whole_task_{}", i))
            .spawn()?;
        tasks.push(taskref);
    }

    for t in &tasks {
        t.join()?;
    }

    let end = hpet.get_counter();
    let hpet_period = hpet.counter_period_femtoseconds();

    println!("Completed runqueue WHOLE evaluation.");
    let elapsed_ticks = end - start - overhead;
    let elapsed_time = hpet_2_us(elapsed_ticks);

    println!("Elapsed HPET ticks: {}, (HPET Period: {} femtoseconds)", 
        elapsed_ticks, hpet_period);
    println!("Elapsed time:{} us", elapsed_time);

    Ok(())
}

fn run_single(iterations: usize) -> Result<(), &'static str> {
    println!("Evaluating runqueue {} with SINGLE tasks, {} iterations...", CONFIG, iterations);
    let overhead = hpet_timing_overhead()?;
    let mut task = Task::new(
        None,
        None,
        |_, _| loop { }, // dummy failure function
    )?;
    task.name = String::from("rq_eval_single_task_unrunnable");
    let taskref = TaskRef::create(task);
    
    let hpet = get_hpet().ok_or("couldn't get HPET timer")?;
    let start = hpet.get_counter();
    
    for _ in 0..iterations {
        runqueue::add_task_to_specific_runqueue(cpu::current_cpu(), taskref.clone())?;
        runqueue::remove_task_from_all(&taskref)?;
    }

    let end = hpet.get_counter();
    let hpet_period = hpet.counter_period_femtoseconds();
    let elapsed_ticks = end - start - overhead;
    let elapsed_time = hpet_2_us(elapsed_ticks);

    println!("Completed runqueue SINGLE evaluation.");
    println!("Elapsed HPET ticks: {}, (HPET Period: {} femtoseconds)", 
        elapsed_ticks, hpet_period);
    println!("Elapsed time:{} us", elapsed_time);

    Ok(())
}


fn whole_task(task_num: usize) -> usize {
    #[cfg(not(rq_eval))]
    warn!("in whole_task, task {}.", task_num);
    task_num
}


fn print_usage(opts: &Options) {
    println!("{}", opts.usage(USAGE));
}


const USAGE: &'static str = "Usage: rq_eval [ARGS]
Evaluates the runqueue implementation.";
