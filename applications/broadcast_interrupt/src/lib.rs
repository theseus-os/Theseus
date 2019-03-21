//! This application is an example of how to write applications in Theseus.

#![no_std]
#![feature(alloc)]

extern crate alloc;
#[macro_use] extern crate terminal_print;
extern crate getopts;
extern crate apic;
extern crate ioapic;
extern crate spawn;

use alloc::vec::Vec;
use alloc::string::String;
use getopts::Options;
use apic::{get_apic_with_id, get_my_apic, get_my_apic_id, LapicIpiDestination, send_interrupt};
use ioapic::{get_ioapics};
use spawn::KernelTaskBuilder;

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

    //find size of ioapic
    // for ioapic in get_ioapics().iter() {
    //     let mut ioapic_ref = ioapic.1.lock();
    //     println!{"IOAPIC: {:#X}", ioapic_ref.read_reg(0x01)};
    // }

    // let mut apic_id = 0;

    // for task_id_str in matches.free.iter() {
    //     match task_id_str.parse::<u8>(){
    //         Ok(task_id) => {
    //             apic_id = task_id;
    //         }, 
    //         _ => { 
    //             println!("Invalid argument {}, not a valid LAPIC ID (u8)", task_id_str); 
    //             return -1;
    //         },
    //     };   
    // }
    let apic_id = matches.free[0].parse::<u8>().unwrap();
    let dest_id = matches.free[1].parse::<u8>().unwrap();

    let child = KernelTaskBuilder::new(send_interrupt, dest_id)
	        .name(String::from("send_interrupt"))
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
