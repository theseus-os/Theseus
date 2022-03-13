#![no_std]

#[macro_use] extern crate log;
extern crate alloc;
extern crate spawn;
extern crate sleep;
extern crate scheduler;

use core::sync::atomic::AtomicUsize;
use alloc::{
    vec::Vec,
    string::String
};

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
        info!("Realtime scheduler not enabled!");
    }
    0
}    

fn task_delay_tester(arg: usize) {
    let start_time : AtomicUsize = AtomicUsize::new(sleep::get_current_time_in_ticks());
    loop {
        info!("I run periodically!");

        // This desk will sleep periodically for 1000 systicks
        sleep::sleep_periodic(&start_time, 1000);
    }
}

