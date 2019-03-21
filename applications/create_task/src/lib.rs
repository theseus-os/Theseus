//! This application is an example of how to write applications in Theseus.

#![no_std]
#![feature(alloc)]

extern crate alloc;
#[macro_use] extern crate terminal_print;
extern crate getopts;
extern crate apic;
extern crate ioapic;
extern crate spawn;
extern crate interrupts;

use alloc::vec::Vec;
use alloc::string::String;
use getopts::Options;
use apic::{get_apic_with_id, get_my_apic, get_my_apic_id, LapicIpiDestination, send_interrupt, print_irr_isr, busy_task};
use ioapic::{get_ioapics};
use spawn::KernelTaskBuilder;
use interrupts::get_timer;

#[no_mangle]
pub fn main(args: Vec<String>) -> isize {

    let mut opts = Options::new();
    
    opts.optflag("h", "help", "print this help menu");
    
    let matches = match opts.parse(&args) {
        Ok(m) => m,
        Err(_f) => {
            println!("{}", _f);
            return -1; 
        }
    };
    
    if matches.opt_present("h") {
        return print_usage(opts);
    }

    let apic_id = matches.free[0].parse::<u8>().unwrap();

    let child = KernelTaskBuilder::new(busy_task, ())
	        .name(String::from("busy_task"))
            .pin_on_core(apic_id)
            .spawn();
    

    0
}

fn print_usage(opts: Options) -> isize{
    println!("{}", opts.usage(USAGE));
    -1
}


const USAGE: &'static str = "Usage: broadcast_interrupt [ARGS] \"lapic_id\"
An example application that just echoes its arguments.";
