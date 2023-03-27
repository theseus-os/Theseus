//! This application is an example of how to collect samples at from the PMU.
//! The application 'pmu_sample_start' should have been called before running this application.

#![no_std]

extern crate alloc;
#[macro_use] extern crate app_io;
extern crate getopts;
extern crate pmu_x86;

use alloc::vec::Vec;
use alloc::string::String;

pub fn main(_args: Vec<String>) -> isize {
    let sampler = pmu_x86::retrieve_samples();
    if let Ok(my_sampler) = sampler {
        pmu_x86::print_samples(&my_sampler);
        if let Err(e) = pmu_x86::find_function_names_from_samples(&my_sampler) {
            println!("Error finding function names from samples: {:?}", e);
        }
    } 
    else {
        println!("Could not retrieve samples");
        return -1;
    }

    0
}

