//! This application demonstrates how to create a periodic task using the `sleep::sleep_periodic` API and add it to the realtime scheduler.

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

/// Main method to ensure the functionality is only tested if the `realtime_scheduler` option is enabled.
/// One potential direction from future testing could be the following:
/// Let the program take in arguments n and p1, ..., pm. 
/// For each pi, we spawn a task with period pi, then instead of the logging statement, we use `hpet()` to measure the time elapsed between successive calls and let each of the spawned tasks run for n periods. 
/// We can use these measurements to get statistics on how much the time between successive calls of the timing statement deviates from the expected time, i.e. the period pi.
pub fn main(_args: Vec<String>) -> isize {
    if cfg!(realtime_scheduler) {
        // build and spawn two real time periodic tasks
        // we will start them as blocked in order to set the periods before they run
        let periodic_tb1 = spawn::new_task_builder(task_delay_tester, 1).block();
        let periodic_task_1 = periodic_tb1.spawn().unwrap();        // setting the periods of the tasks

        scheduler::set_periodicity(&periodic_task_1, 1000).unwrap();

        // starting the tasks
        periodic_task_1.unblock();
    }
    else {
        println!("Realtime scheduler not enabled!");
        return -1;
    }
    0
}    

/// A simple periodic task using the `sleep_periodic` API that will log a string at regular intervals.
fn task_delay_tester(_arg: usize) {
    let start_time : AtomicUsize = AtomicUsize::new(sleep::get_current_time_in_ticks());
    loop {
        info!("I run periodically!");

        // This desk will sleep periodically for 1000 systicks
        sleep::sleep_periodic(&start_time, 1000);
    }
}

