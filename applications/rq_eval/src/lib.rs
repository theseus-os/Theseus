//! This application tests the performance of the runqueue implementation,
//! which is used to compare a standard runqueue with a state spill-free runqueue.

#![no_std]

#[macro_use] extern crate alloc;
#[macro_use] extern crate terminal_print;
extern crate log;
extern crate task;
extern crate spawn;
extern crate runqueue;
extern crate getopts;
extern crate hpet;

use alloc::string::String;
use alloc::vec::Vec;
use getopts::{Matches, Options};
use hpet::get_hpet;
use task::{Task, TaskRef};



#[cfg(runqueue_state_spill_evaluation)]
const CONFIG: &'static str = "WITH state spill";
#[cfg(not(runqueue_state_spill_evaluation))]
const CONFIG: &'static str = "WITHOUT state spill";

const _FEMTOSECONDS_PER_SECOND: u64 = 1000*1000*1000*1000*1000; // 10^15


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
    let start = get_hpet().as_ref().ok_or("couldn't get HPET timer")?.get_counter();
    
    for i in 0..num_tasks {
        let taskref = spawn::KernelTaskBuilder::new(whole_task, i)
            .name(format!("rq_whole_task_{}", i))
            .spawn()?;
        taskref.join()?;
        let _ = taskref.take_exit_value();
    }

    let end = get_hpet().as_ref().ok_or("couldn't get HPET timer")?.get_counter();
    let hpet_period = get_hpet().as_ref().ok_or("couldn't get HPET timer")?.counter_period_femtoseconds();

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

    let start = get_hpet().as_ref().ok_or("couldn't get HPET timer")?.get_counter();
    
    for _i in 0..iterations {
        runqueue::add_task_to_any_runqueue(taskref.clone())?;

        #[cfg(runqueue_state_spill_evaluation)] 
        {   
            let task_on_rq = { taskref.lock().on_runqueue.clone() };
            if let Some(remove_from_runqueue) = task::RUNQUEUE_REMOVAL_FUNCTION.try() {
                if let Some(rq) = task_on_rq {
                    remove_from_runqueue(&taskref, rq)?;
                }
            }
        }
        #[cfg(not(runqueue_state_spill_evaluation))]
        {
            runqueue::remove_task_from_all(&taskref)?;
        }
    }

    let end = get_hpet().as_ref().ok_or("couldn't get HPET timer")?.get_counter();
    let hpet_period = get_hpet().as_ref().ok_or("couldn't get HPET timer")?.counter_period_femtoseconds();

    println!("Completed runqueue SINGLE evaluation.");
    let elapsed_ticks = end - start;
    println!("Elapsed HPET ticks: {}, (HPET Period: {} femtoseconds)", 
        elapsed_ticks, hpet_period);

    Ok(())
}


fn whole_task(task_num: usize) -> usize {
    // warn!("in whole_task, task {}.", task_num);
    task_num
}


fn print_usage(opts: &Options) {
    println!("{}", opts.usage(USAGE));
}


const USAGE: &'static str = "Usage: rq_eval [ARGS]
Evaluates the runqueue implementation.";
