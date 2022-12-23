#![no_std]

#[macro_use] extern crate alloc;
#[macro_use] extern crate log;
extern crate task;
extern crate spawn;
extern crate scheduler;
extern crate mutex_sleep;
extern crate cpu;

use core::ops::Deref;

use alloc::{
    vec::Vec,
    string::String,
    sync::Arc,
};
use mutex_sleep::MutexSleep;


pub fn main(_args: Vec<String>) -> isize {    
    let res = match _args.get(0).map(|s| &**s) {
        Some("-c") => test_contention(),
        _          => test_lockstep(),
    };
    match res {
        Ok(_) => 0,
        Err(e) => {
            error!("Error: {}", e); 
            -1
        }
    }
    
}

/// A simple test that spawns 3 tasks that all contend to increment a shared usize
fn test_contention() -> Result<(), &'static str> {
    let my_cpu = cpu::current_cpu();

    let shared_lock = Arc::new(MutexSleep::new(0usize));

    let t1 = spawn::new_task_builder(mutex_sleep_task, shared_lock.clone())
        .name(String::from("mutex_sleep_test_1"))
        .pin_on_core(my_cpu)
        .block()
        .spawn()?;

    let t2 = spawn::new_task_builder(mutex_sleep_task, shared_lock.clone())
        .name(String::from("mutex_sleep_test_2"))
        .pin_on_core(my_cpu)
        .block()
        .spawn()?;
    
    let t3 = spawn::new_task_builder(mutex_sleep_task, shared_lock.clone())
        .name(String::from("mutex_sleep_test_3"))
        .pin_on_core(my_cpu)
        .block()
        .spawn()?;

    warn!("Finished spawning the 3 tasks");

    t3.unblock().unwrap();
    t2.unblock().unwrap();
    t1.unblock().unwrap();

    t1.join()?;
    t2.join()?;
    t3.join()?;
    warn!("Joined the 3 tasks. Final value of shared_lock: {:?}", shared_lock);
    
    Ok(())
}


fn mutex_sleep_task(lock: Arc<MutexSleep<usize>>) -> Result<(), &'static str> {
    let curr_task = task::with_current_task(|t| format!("{}", t.deref()))
        .map_err(|_| "couldn't get current task")?;
    warn!("ENTERED TASK {}", curr_task);

    for _i in 0..1000 {
        scheduler::schedule(); // give other tasks a chance to acquire the lock
        warn!("{} trying to acquire lock...", curr_task);
        let mut locked = lock.lock()?;
        warn!("{} acquired lock!", curr_task);
        *locked += 1;
        warn!("{} incremented shared_lock value to {}.  Releasing lock.", curr_task, &*locked);
    }
    warn!("{} \n     FINISHED LOOP.", curr_task);
    Ok(())
}



/// A test for running multiple tasks that are synchronized in lockstep
fn test_lockstep() -> Result<(), &'static str> {
    let my_cpu = cpu::current_cpu();

    let shared_lock = Arc::new(MutexSleep::new(0usize));

    let t1 = spawn::new_task_builder(lockstep_task, (shared_lock.clone(), 0))
        .name(String::from("lockstep_task_1"))
        .pin_on_core(my_cpu)
        .block()
        .spawn()?;

    let t2 = spawn::new_task_builder(lockstep_task, (shared_lock.clone(), 1))
        .name(String::from("lockstep_task_2"))
        .pin_on_core(my_cpu)
        .block()
        .spawn()?;
    
    let t3 = spawn::new_task_builder(lockstep_task, (shared_lock.clone(), 2))
        .name(String::from("lockstep_task_3"))
        .pin_on_core(my_cpu)
        .block()
        .spawn()?;

    warn!("Finished spawning the 3 tasks");

    t3.unblock().unwrap();
    t2.unblock().unwrap();
    t1.unblock().unwrap();

    t1.join()?;
    t2.join()?;
    t3.join()?;
    warn!("Joined the 3 tasks. Final value of shared_lock: {:?}", shared_lock);
    
    Ok(())
}


fn lockstep_task((lock, remainder): (Arc<MutexSleep<usize>>, usize)) -> Result<(), &'static str> {
    let curr_task = task::with_current_task(|t| format!("{}", t.deref()))
        .map_err(|_| "couldn't get current task")?;
    warn!("ENTERED TASK {}", curr_task);

    for _i in 0..20 {
        loop { 
            warn!("{} top of loop, remainder {}", curr_task, remainder);
            scheduler::schedule(); // give other tasks a chance to acquire the lock
            let mut locked = lock.lock()?;
            scheduler::schedule();
            if *locked % 3 == remainder {
                warn!("Task {} Time to shine, value is {}!", curr_task, *locked);
                *locked += 1;
                break;
            } else {
                scheduler::schedule();
                warn!("Task {} going back to sleep, value {}, remainder {}!", curr_task, *locked, remainder);
            }
            scheduler::schedule();
        }
    }
    warn!("{} finished loop.", curr_task);
    Ok(())
}
