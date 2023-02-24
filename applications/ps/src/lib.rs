#![no_std]
extern crate alloc;
#[macro_use] extern crate app_io;

extern crate task;
extern crate getopts;
extern crate scheduler;

use getopts::Options;
use alloc::vec::Vec;
use alloc::string::String;
use core::fmt::Write;
use task::TASKLIST;

pub fn main(args: Vec<String>) -> isize {
    let mut opts = Options::new();
    opts.optflag("h", "help", "print this help menu");
    opts.optflag("b", "brief", "print only task id and name");

    let matches = match opts.parse(&args) {
        Ok(m) => m,
        Err(_f) => { 
            println!("{} \n", _f);
            return -1; 
        }
    };

    if matches.opt_present("h") {
        return print_usage(opts)
    }

    // Print headers
    if matches.opt_present("b") {
        println!("{0:<5}  {1}", "ID", "NAME");
    }
    else {
        #[cfg(priority_scheduler)] {
            println!("{0:<5}  {1:<10}  {2:<4}  {3:<4}  {4:<5}  {5:<10}  {6}", "ID", "RUNSTATE", "CPU", "PIN", "TYPE", "PRIORITY", "NAME");
        }
        #[cfg(not(priority_scheduler))] {
            println!("{0:<5}  {1:<10}  {2:<4}  {3:<4}  {4:<5}  {5}", "ID", "RUNSTATE", "CPU", "PIN", "TYPE", "NAME");
        }
    }

    // Print all tasks
    let mut num_tasks = 0;
    let mut task_string = String::new();
    for (id, task) in TASKLIST.lock().iter() {
        num_tasks += 1;
        if matches.opt_present("b") {
            writeln!(task_string, "{0:<5}  {1}", id, task.name).expect("Failed to write to task_string.");
        }
        else {
            // All printed fields below must be strings to ensure the width formatting specifier below works properly.
            let runstate = format!("{:?}", task.runstate());
            let cpu = task.running_on_cpu().map(|cpu| format!("{cpu}")).unwrap_or_else(|| String::from("-"));
            let pinned = task.pinned_core().map(|pin| format!("{pin}")).unwrap_or_else(|| String::from("-"));
            let task_type = if task.is_an_idle_task {"I"}
                else if task.is_application() {"A"}
                else {" "} ;

            #[cfg(priority_scheduler)] {
                let priority = scheduler::get_priority(&task).map(|priority| format!("{}", priority)).unwrap_or_else(|| String::from("-"));
                task_string.push_str(
                    &format!("{0:<5}  {1:<10}  {2:<4}  {3:<4}  {4:<5}  {5:<10}  {6}\n", 
                    id, runstate, cpu, pinned, task_type, priority, task.name)
                );
            }
            #[cfg(not(priority_scheduler))] {
                writeln!(task_string, "{0:<5}  {1:<10}  {2:<4}  {3:<4}  {4:<5}  {5}", 
                    id, runstate, cpu, pinned, task_type, task.name).expect("Failed to write to task_string.");
            }
        }
    }
    print!("{}", task_string);
    println!("Total number of tasks: {}", num_tasks);
    
    0
}

fn print_usage(opts: Options) -> isize {
    println!("{}", opts.usage(BRIEF));
    0
}

const BRIEF: &str = "Usage: ps [options]\n
    TYPE:      'I' if an idle task, 'A' if an application task, '-' otherwise.
    CPU:       the cpu core the task is currently running on.
    PIN:       the core the task is pinned on, if any.
    RUNSTATE:  runnability status of this task, e.g., whether it can be scheduled in.
    ID:        the unique identifier for this task.
    NAME:      the name of the task.";
    