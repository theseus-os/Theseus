#![no_std]
#![feature(alloc)]
#[macro_use] extern crate alloc;
#[macro_use] extern crate input_event_manager;

extern crate apic;
extern crate getopts;
extern crate scheduler;

use getopts::Options;
use alloc::{Vec, String};
use apic::get_lapics;
use scheduler::get_runqueue;

#[no_mangle]
pub fn main(args: Vec<String>) -> isize {
    let mut opts = Options::new();
    opts.optflag("h", "help", "print this help menu");

    let matches = match opts.parse(&args) {
        Ok(m) => { m }
        Err(_f) => { println!("{} \n", _f);
                    return -1; }
    };

    if matches.opt_present("h") {
        return print_usage(opts)
    }

    let all_lapics = get_lapics();
    for lapic in all_lapics.iter() {
        let lapic = lapic.1;
        let apic_id = lapic.read().apic_id;
        let processor = lapic.read().processor;
        let is_bsp = lapic.read().is_bsp;
        let core_type = if is_bsp {"BSP Core"}
                        else {"AP Core"};

        println!("{} (apic: {}, proc: {})", core_type, apic_id, processor); 
        
        if let Some(runqueue) = get_runqueue(apic_id) {
            let mut currently_running = String::new();
            let mut on_runqueue = String::new();
            for task_ref in runqueue.iter() {
                if task_ref.read().running_on_cpu < 0 {
                    on_runqueue.push_str(&task_ref.read().name);
                    on_runqueue.push('\n');
                }
                if task_ref.read().running_on_cpu == apic_id as isize {
                    currently_running.push_str(&task_ref.read().name);
                }
            }
            print!("Task: {}\n", currently_running);
            println!("Runqueue:\n{}",on_runqueue);
        }
        
        else {
            println!("Can't retrieve runqueue");
            return -1;
        }
    }
    
    0
}

fn print_usage(opts: Options) -> isize {
    let mut brief = format!("Usage: cpu \n \n");

    brief.push_str("For each core, prints apic id, processor id, whether it is the bootstrap processor (the first processor to boot up), which tasks that is currently running on that core and which tasks are present in that core's runqueue");

    println!("{} \n", opts.usage(&brief));

    0
}
