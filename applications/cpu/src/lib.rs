#![no_std]
#[macro_use] extern crate alloc;
#[macro_use] extern crate terminal_print;

extern crate apic;
extern crate getopts;
extern crate task;
extern crate runqueue;

use getopts::Options;
use alloc::vec::Vec;
use alloc::string::String;
use apic::get_lapics;

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
        let apic_id = lapic.read().apic_id();
        let processor = lapic.read().processor();
        let is_bsp = lapic.read().is_bsp();
        let core_type = if is_bsp {"BSP Core"}
                        else {"AP Core"};

        println!("\n{} (apic: {}, proc: {})", core_type, apic_id, processor); 
        
        if let Some(runqueue) = runqueue::get_runqueue(apic_id).map(|rq| rq.read()) {
            let mut runqueue_contents = String::new();
            for task in runqueue.iter() {
                runqueue_contents.push_str(&format!("{} ({}) {}\n", 
                    task.name, 
                    task.id,
                    if task.is_running() { "*" } else { "" },
                ));
            }
            println!("RunQueue:\n{}", runqueue_contents);
        }
        
        else {
            println!("Can't retrieve runqueue for core {}", apic_id);
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
