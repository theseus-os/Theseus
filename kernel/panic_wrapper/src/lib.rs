//! Provides types and simple routines for handling panics.
//! This is similar to what's found in Rust's `core::panic` crate, 
//! but is much simpler and doesn't require lifetimes 
//! (although it does require alloc types like String).
//! 
#![no_std]
#![feature(alloc)]

extern crate alloc;
#[macro_use] extern crate log;
extern crate panic_info;
extern crate memory;
extern crate apic;
extern crate task;


use alloc::String;
use panic_info::{PanicInfo, PanicLocation};
use task::KillReason;


/// performs the standard panic handling routine, which involves the following:
/// 
/// * printing a basic panic message.
/// * getting the current `Task`.
/// * invoking the current `Task`'s `panic_handler` routine, if it has registered one.
/// * if there is no registered panic handler, then it prints a standard message plus a stack backtrace.
/// * Finally, it kills the panicked `Task`.
/// 
pub fn panic_wrapper(fmt_args: core::fmt::Arguments, file: &'static str, line: u32, col: u32) -> Result<(), &'static str> {
    trace!("at top of panic_wrapper: {} {}:{}:{}", fmt_args, file, line, col);

    let apic_id = apic::get_my_apic_id();
    let panic_info = PanicInfo::with_fmt_args(PanicLocation::new(file, line, col), fmt_args);

    // get current task to see if it has a panic_handler
    let curr_task = task::get_my_current_task();

    let curr_task_name = curr_task.map(|t| t.read().name.clone()).unwrap_or(String::from("UNKNOWN!"));
    let curr_task = curr_task.ok_or("get_my_current_task() failed")?;
    
    // call this task's panic handler, if it has one. 
    let panic_handler = { 
        curr_task.write().take_panic_handler()
    };
    if let Some(ref ph_func) = panic_handler {
        ph_func(&panic_info);
        error!("PANIC handled in task \"{}\" on core {:?} at {} -- {}", curr_task_name, apic_id, panic_info.location, panic_info.msg);
    }
    else {
        error!("PANIC was unhandled in task \"{}\" on core {:?} at {} -- {}", curr_task_name, apic_id, panic_info.location, panic_info.msg);
        memory::stack_trace();
    }

    if !curr_task.read().is_an_idle_task() {
        // kill the offending task (the current task)
        error!("Killing panicked task \"{}\"", curr_task_name);
        curr_task.kill(KillReason::Panic(panic_info))?;
    }
    
    // scheduler::schedule(); // yield the current task after it's done

    Ok(())
}