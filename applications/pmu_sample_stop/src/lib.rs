//! This application is an example of how to write applications in Theseus.

#![no_std]

extern crate alloc;
#[macro_use] extern crate print;
extern crate getopts;
#[cfg(target_arch = "x86_64")]
extern crate pmu_x86;

use alloc::vec::Vec;
use alloc::string::String;
use getopts::Options;


#[no_mangle]
pub fn main(args: Vec<String>) -> isize {
    let mut opts = Options::new();
    opts.optflag("h", "help", "print this help menu");

    let matches = match opts.parse(&args) {
        Ok(m) => m,
        Err(_f) => {
            println!("{}", _f);
            print_usage(opts);
            return -1; 
        }
    };
    println!("We are indeed here!");
    //let sampler = pmu_x86::start_samples(pmu_x86::EventType::UnhaltedReferenceCycles, 0xFFFFF, None, 150);
    #[cfg(target_arch = "x86_64")]
    {
        let sampler = pmu_x86::retrieve_samples();
        if let Ok(mut my_sampler) = sampler {
            pmu_x86::print_samples(&mut my_sampler);
            /*
            println!("Sampling running ok.");
            let mut counter = 0;
            while counter < 300 {
                println!("{}", counter);
                counter += 1;
            } 
            */
            /*
            if let Ok(mut samples) = pmu_x86::retrieve_samples() {
                println!("The results from retrieve_samples was okay");
                pmu_x86::print_samples(&mut samples);
            } else {
                println!("Something went wrong!");
            }
            */
        } else {
            println!("Sample didn't begin");
        }
    }
    
    #[cfg(target_arch = "aarch64")]
    {
        // TODO
    }
    
    if matches.opt_present("h") {
        print_usage(opts);
        return 0;
    }

    println!("This is an example application.\nArguments: {:?}", args);

    0
}



fn print_usage(opts: Options) {
    println!("{}", opts.usage(USAGE));
}


const USAGE: &'static str = "Usage: example [ARGS]
An example application that just echoes its arguments.";
