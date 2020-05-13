//! This application tests the performance of the runqueue implementation,
//! which is used to compare a standard runqueue with a state spill-free runqueue.
//! 
//! # Instructions for Running
//! When running experiments, enable the proper configs:
//! * For the state spill-free (regular) version, use THESEUS_CONFIG="rq_eval"
//! * For the state spillful version, use THESEUS_CONFIG="rq_eval runqueue_spillful"
//! You should do a clean build in between each one.
//! 
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
#[macro_use] extern crate alloc;
#[macro_use] extern crate terminal_print;
extern crate task;
extern crate apic;
extern crate spawn;
extern crate runqueue;
extern crate getopts;
extern crate hpet;

use alloc::{
    boxed::Box,
    string::String,
    vec::Vec,
};
use getopts::{Matches, Options};
use hpet::get_hpet;
use task::{Task, TaskRef};



#[cfg(runqueue_spillful)]
const CONFIG: &'static str = "WITH state spill";
#[cfg(not(runqueue_spillful))]
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
    let hpet = get_hpet().ok_or("couldn't get HPET timer")?;
    let start = hpet.get_counter();
    
    for i in 0..num_tasks {
        let taskref = spawn::new_task_builder(whole_task, i)
            .name(format!("rq_whole_task_{}", i))
            .spawn()?;
        taskref.join()?;
        let _ = taskref.take_exit_value();
    }

    let end = hpet.get_counter();
    let hpet_period = hpet.counter_period_femtoseconds();

    println!("Completed runqueue WHOLE evaluation.");
    let elapsed_ticks = end - start;
    println!("Elapsed HPET ticks: {}, (HPET Period: {} femtoseconds)", 
        elapsed_ticks, hpet_period);
        
    Ok(())
}


fn run_single(iterations: usize) -> Result<(), &'static str> {
    println!("Evaluating runqueue {} with SINGLE tasks, {} iterations...", CONFIG, iterations);
    let mut task = Task::new(
        None,
        |_, _| loop { }, // dummy failure function
    )?;
    task.name = String::from("rq_eval_single_task_unrunnable");
    let taskref = TaskRef::new(task);
    
    let hpet = get_hpet().ok_or("couldn't get HPET timer")?;
    let start = hpet.get_counter();
    
    for _i in 0..iterations {
        runqueue::add_task_to_specific_runqueue(apic::get_my_apic_id(), taskref.clone())?;

        #[cfg(runqueue_spillful)] 
        {   
            let task_on_rq = { taskref.lock().on_runqueue.clone() };
            if let Some(remove_from_runqueue) = task::RUNQUEUE_REMOVAL_FUNCTION.try() {
                if let Some(rq) = task_on_rq {
                    remove_from_runqueue(&taskref, rq)?;
                }
            }
        }
        #[cfg(not(runqueue_spillful))]
        {
            runqueue::remove_task_from_all(&taskref)?;
        }
    }

    let end = hpet.get_counter();
    let hpet_period = hpet.counter_period_femtoseconds();

    println!("Completed runqueue SINGLE evaluation.");
    let elapsed_ticks = end - start;
    println!("Elapsed HPET ticks: {}, (HPET Period: {} femtoseconds)", 
        elapsed_ticks, hpet_period);

    // cleanup the dummy task we created earlier
    taskref.mark_as_exited(Box::new(0usize))?;
    taskref.take_exit_value();
    
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
