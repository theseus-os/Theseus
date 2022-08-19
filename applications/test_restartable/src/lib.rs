//! This application tests the restartable tasks in the presence
//! of graceful exit, panic and exceptions.

#![no_std]

#[macro_use] extern crate log;
#[macro_use] extern crate terminal_print;
extern crate getopts;
extern crate alloc;
extern crate spawn;
extern crate spin;


use spin::Mutex;
use alloc::vec::Vec;
use alloc::string::String;
use getopts::Options;
use spawn::new_task_builder;
use core::ptr;

/// A simple struct to verify dropping of objects.
struct DropStruct {
    index : usize
}

impl Drop for DropStruct {
    fn drop(&mut self) {
        warn!("DROPPING the lock {}", self.index);
    }
}

/// A lock to check and verify releasing of locks
static STATIC_LOCK: Mutex<Vec<usize>> = Mutex::new(Vec::new());

/// An enum to hold the methods we plan to exit a task
#[derive(Clone)]
enum ExitMethod {
    Graceful,
    Panic,
    Exception,
}

#[inline(never)]
fn simple_restartable_loop(exit_method: ExitMethod) -> Result<(), &'static str> {
    match exit_method {
        ExitMethod::Graceful => {
            debug!("Hi, I'm restartable function");
        },
        ExitMethod::Panic => {
            debug!("Hi, I'm restartable function with panic");
            panic!("paniced");
        },
        ExitMethod::Exception => {
            debug!("Hi, I'm restartable function with exception");
            #[cfg(not(unwind_exceptions))]{
                debug!("Will not restart as it is compiled without unwind_exceptions directive");
            }
            // causes a page fault
            unsafe {*(0x5050DEADBEEF as *mut usize) = 0x5555_5555_5555;}
        },
    }
    Ok(())   
}

/// A restartable task where the lock is released
fn restartable_loop_with_lock(exit_method: ExitMethod) -> Result<(), &'static str> {
    debug!("Running a restart loop with a lock");
    let mut te = STATIC_LOCK.lock();
    simple_restartable_loop(exit_method)?;
    te.push(3);
    return Ok(());
}

/// A restartable task where the lock is failed to release. 
/// This is due to no entry point between lock acquiring and fault occuring. 
fn restartable_lock_fail(_exit_method: ExitMethod) -> Result<(), &'static str> {
    debug!("Running a restart loop with undroppable lock");
    let mut te = STATIC_LOCK.lock();
    let x :usize = 0x5050DEADBEEF;
    let p = (x) as *const u64;
    let n = unsafe{ptr::read(p)};
    debug!("unsafe value is {:X}",n);
    te.push(3);
    return Ok(());
}

/// Same as `restartable_lock_fail` but the lock will be released due to 
/// entry point caused by calling `recursive_add_fault`.
fn restartable_lock_modified(_exit_method: ExitMethod) -> Result<(), &'static str> {
    debug!("Running a restart loop with a droppable");
    let mut te = STATIC_LOCK.lock();
    recursive_add_fault(ExitMethod::Graceful, 3);
    let x :usize = 0x5050DEADBEEF;
    let p = (x) as *const u64;
    let n = unsafe{ptr::read(p)};
    debug!("unsafe value is {:X}",n);
    te.push(3);
    return Ok(());
}

/// A restartable function to call `recursive_ add_fault`
fn restartable_lock_recursive(exit_method: ExitMethod) -> Result<(), &'static str> {
    debug!("Recurive function with holding multiple objects");
    let mut te = STATIC_LOCK.lock();
    let a = recursive_add_fault(exit_method, 4);
    te.push(a.index);
    return Ok(());
}

/// A recurisive function acquiring structures and will be dropping them 
/// during unwinding
#[inline(never)]
fn recursive_add_fault(exit_method: ExitMethod, i: usize) -> DropStruct{
    if i==0 {
        match exit_method {
            ExitMethod::Graceful => {},
            ExitMethod::Panic => {
                panic!("paniced");
            },
            ExitMethod::Exception => {
                debug!("Exception occured");
                #[cfg(not(unwind_exceptions))]{
                    debug!("Will not restart as it is compiled without unwind_exceptions directive");
                }
                // causes a page fault
                unsafe {*(0x5050DEADBEEF as *mut usize) = 0x5555_5555_5555;}
            },
        }
        let drop_struct = DropStruct{index : i};
        return drop_struct
    } else {
        let mut drop_struct = DropStruct{index : i};
        drop_struct.index = recursive_add_fault(exit_method, i-1).index + i;
        drop_struct
    }
}
    
pub fn main(args: Vec<String>) -> isize {
    let mut opts = Options::new();
    opts.optflag("p", "panic", "induce panic to restartable task");
    opts.optflag("x", "exception", "induce exception to restartable task");

    opts.optflag("h", "help", "print this help menu");
    opts.optflag("s", "simple", "runs a simple restartable task");
    opts.optflag("l", "lock", "runs a simple restartable task with lock");
    opts.optflag("f", "fail", "runs a simple restartable task with lock but fails to unlock");
    opts.optflag("m", "modified", "same as failed but with a simple modification to unlock properly");
    opts.optflag("r", "recursive", "runs a recursive function");

    let matches = match opts.parse(&args) {
        Ok(m) => m,
        Err(_f) => {
            println!("{}", _f);
            print_usage(opts);
            return -1; 
        }
    };

    let mut exit_method = ExitMethod::Graceful;

    if matches.opt_present("h") {
        print_usage(opts);
        return 0;
    }

    if matches.opt_present("p") {
        exit_method = ExitMethod::Panic;
    }

    if matches.opt_present("x") {
        exit_method = ExitMethod::Exception;
    }


    if matches.opt_present("s"){
        let taskref1  = new_task_builder(simple_restartable_loop, exit_method)
            .name(String::from("restartable_loop"))
            .spawn_restartable(None)
            .expect("Couldn't start the restartable task"); 

        taskref1.join().expect("Task 1 join failed");
    } else if matches.opt_present("l"){
        let taskref1  = new_task_builder(restartable_loop_with_lock, exit_method)
            .name(String::from("restartable_loop with lock"))
            .spawn_restartable(None)
            .expect("Couldn't start the restartable task"); 

        taskref1.join().expect("Task 1 join failed");
    } else if matches.opt_present("f"){
        let taskref1  = new_task_builder(restartable_lock_fail, exit_method)
            .name(String::from("restartable_lock_fail"))
            .spawn_restartable(None)
            .expect("Couldn't start the restartable task"); 

        taskref1.join().expect("Task 1 join failed");
    } else if matches.opt_present("m"){
        let taskref1  = new_task_builder(restartable_lock_modified, exit_method)
            .name(String::from("restartable_lock_fail"))
            .spawn_restartable(None)
            .expect("Couldn't start the restartable task"); 

        taskref1.join().expect("Task 1 join failed");
    } else if matches.opt_present("r"){
        let taskref1  = new_task_builder(restartable_lock_recursive, exit_method)
            .name(String::from("restartable_lock_fail"))
            .spawn_restartable(None)
            .expect("Couldn't start the restartable task"); 

        taskref1.join().expect("Task 1 join failed");
    }

    return 0;
}

fn print_usage(opts: Options) {
    println!("{}", opts.usage(USAGE));
}

const USAGE: &'static str = "Usage: test_restartable [OPTION] ARG
Spawns a simple restartable task that can encounter panic and exceptions.";
