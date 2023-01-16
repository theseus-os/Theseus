//! This application is an example of how to start event based sampling using the PMU

#![no_std]

extern crate alloc;
#[macro_use] extern crate app_io;
extern crate getopts;
extern crate pmu_x86;
extern crate spawn;

use alloc::vec::Vec;
use alloc::string::String;

pub fn main(_args: Vec<String>) -> isize {
    if let Err(e) = pmu_x86::init() {
        println!("Could not initialize PMU: {:?}", e);
        return -1;
    }

    // run the pmu stat on this core
    /*let pmu = pmu_x86::stat::PerformanceCounters::new();
    match pmu {
        Ok(mut x) => {
            match x.end(){
                Ok(results) => println!("{:?}", results),
                Err(x) => println!("Results could not be retrieved for PMU stat: {:?}", x)
            }
        },
        Err(x) => println!("Could not create counters for PMU stat: {:?}", x)
    } */
    
    // run event based sampling on this core
    if let Err(e) = pmu_x86::start_samples(pmu_x86::EventType::UnhaltedReferenceCycles, 0xF_FFFF, None, 10) {
        println!("Could not start PMU sampling: {:?}", e);
        return -1;
    }

    0
}
