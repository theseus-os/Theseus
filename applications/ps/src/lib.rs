#![no_std]
#[macro_use] extern crate alloc;
#[macro_use] extern crate terminal_print;

extern crate task;
extern crate getopts;
extern crate scheduler;

use getopts::Options;
use alloc::vec::Vec;
use alloc::string::String;
use task::{TASKLIST, RunState};

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
    for (id, taskref) in TASKLIST.lock().iter() {
        num_tasks += 1;
        let name;
        let runstate;
        let cpu;
        let pinned; 
        let task_type;
        // only hold the task's lock for a short time
        {
            let task = taskref.lock();
            name = task.name.clone();
            runstate = match &task.runstate {
                RunState::Initing    => "Initing",
                RunState::Runnable   => "Runnable",
                RunState::Blocked    => "Blocked",
                RunState::Exited(_)  => "Exited",
                RunState::Reaped     => "Reaped",
            };
            cpu = task.running_on_cpu.map(|cpu| format!("{}", cpu)).unwrap_or_else(|| String::from("-"));
            pinned = task.pinned_core.map(|pin| format!("{}", pin)).unwrap_or_else(|| String::from("-"));
            task_type = if task.is_an_idle_task {"I"}
                else if task.is_application() {"A"}
                else {" "} ;
        }    
        if matches.opt_present("b") {
            task_string.push_str(&format!("{0:<5}  {1}\n", id, name));
        }
        else {

            #[cfg(priority_scheduler)] {
                let priority = scheduler::get_priority(&taskref).map(|priority| format!("{}", priority)).unwrap_or_else(|| String::from("-"));
                task_string.push_str(
                    &format!("{0:<5}  {1:<10}  {2:<4}  {3:<4}  {4:<5}  {5:<10}  {6}\n", 
                    id, runstate, cpu, pinned, task_type, priority, name)
                );
            }
            #[cfg(not(priority_scheduler))] {
                task_string.push_str(
                    &format!("{0:<5}  {1:<10}  {2:<4}  {3:<4}  {4:<5}  {5}\n", 
                    id, runstate, cpu, pinned, task_type, name)
                );
            }
        }
    }
    print!("{}", task_string);
    println!("Total number of tasks: {}", num_tasks);
    
    0
}

fn print_usage(opts: Options) -> isize {
    let mut brief = format!("Usage: ps [options] \n \n");

    brief.push_str("TYPE is 'I' if it is an idle task and 'A' if it is an application task. \n");
    brief.push_str("CPU is the cpu core the task is currently running on. \n");
    brief.push_str("PIN is the core the task is pinned on, if any. \n");
    brief.push_str("RUNSATE is runnability status of this task, i.e. whether it's allowed to be scheduled in. \n");
    brief.push_str("ID is the unique id of task. \n");
    brief.push_str("NAME is the simple name of the task");

    println!("{} \n", opts.usage(&brief));

    0
}