#![no_std]
extern crate alloc;
#[macro_use] extern crate app_io;

extern crate apic;
extern crate getopts;
extern crate task;
extern crate runqueue;

use getopts::Options;
use alloc::{
    fmt::Write,
    string::{
        String,
        ToString,
    },
    vec::Vec,
};
use apic::get_lapics;

pub fn main(args: Vec<String>) -> isize {
    let mut opts = Options::new();
    opts.optflag("h", "help", "print this help menu");

    let matches = match opts.parse(args) {
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
        let processor = lapic.read().processor_id();
        let is_bootstrap_cpu = lapic.read().is_bootstrap_cpu();
        let core_type = if is_bootstrap_cpu { "Boot CPU" } else { "Secondary CPU" };

        println!("\n{} (apic: {}, proc: {})", core_type, apic_id, processor); 
        
        if let Some(runqueue) = runqueue::get_runqueue(apic_id.value() as u8).map(|rq| rq.read().clone()) {
            let mut runqueue_contents = String::new();
            for task in runqueue.iter() {
                writeln!(runqueue_contents, "    {} ({}) {}", 
                    task.name, 
                    task.id,
                    if task.is_running() { "*" } else { "" }
                )
                .expect("Failed to write to runqueue_contents");
            }
            print!("{}", runqueue_contents);
        }
        
        else {
            println!("Can't retrieve runqueue for core {}", apic_id);
            return -1;
        }
    }
    
    println!("");
    0
}

fn print_usage(opts: Options) -> isize {
    let mut brief = "Usage: rq \n \n".to_string();

    brief.push_str("Prints each CPU's ID, the tasks on its runqueue ('*' identifies the currently running task), and whether it is the boot CPU or not");

    println!("{} \n", opts.usage(&brief));

    0
}
