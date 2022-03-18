//! Demo of scheduling a periodic task using the realtime scheduler.
//! 
//! One potential direction for future testing could be the following:
//! Let the program take in arguments `n` and `p1` ... `pm`. 
//! For each `p_i`, we:
//! 1. spawn a task with period `pi`
//! 2. Let each spawned task run for `n` complete periods.
//! 3. Use`hpet()` to measure the time elapsed between successive executions of each task. 
//! 4. Calculate statistics on how much the time between successive calls
//!    to the timing statement deviates from the expected time, i.e. the period `pi`.
//!    * This is one way to assess the accuracy of the `sleep` function.
//!

#![no_std]

#[macro_use] extern crate log;
extern crate alloc;
extern crate spawn;
extern crate sleep;
extern crate scheduler;
#[macro_use] extern crate terminal_print;

use core::sync::atomic::AtomicUsize;
use alloc::{
    vec::Vec,
    string::String
};

pub fn main(_args: Vec<String>) -> isize {
    #[cfg(not(realtime_scheduler))] {
        println!("Error: `realtime_scheduler` cfg was not enabled!");
        -1
    }
    #[cfg(realtime_scheduler)] {
        println!("Testing periodic task(s) with the realtime scheduler!");
        // Build and spawn two real time periodic task(s).
        // Start them as blocked in order to set the periods before they run
        let periodic_tb1 = spawn::new_task_builder(_task_delay_tester, 1).block();
        let periodic_task_1 = periodic_tb1.spawn().unwrap();
        
        // Set the periods of the task
        scheduler::set_periodicity(&periodic_task_1, 1000).unwrap();

        // start the tasks
        periodic_task_1.unblock();

        0
    }
}    

/// A simple task that periodically sleeps and prints a log statement at regular intervals.
fn _task_delay_tester(_arg: usize) {
    let start_time : AtomicUsize = AtomicUsize::new(sleep::get_current_time_in_ticks());
    let mut iter = 0;
    loop {
        info!("I run periodically (iter {}).", iter);
        iter += 1;

        // This desk will sleep periodically for 1000 systicks
        sleep::sleep_periodic(&start_time, 1000);
    }
}
