#![no_std]

extern crate alloc;
#[macro_use] extern crate log;
// #[macro_use] extern crate app_io;
extern crate spin;
extern crate task;
extern crate spawn;
extern crate scheduler;
extern crate wait_condition;
extern crate cpu;

// use core::sync::atomic::{Ordering, AtomicBool};
use alloc::{
    vec::Vec,
    string::String,
    sync::Arc,
};
use spin::Mutex;
use wait_condition::{WaitCondition, WaitConditionFn};


pub fn main(_args: Vec<String>) -> isize {    
    match rmain() {
        Ok(_) => 0,
        Err(e) => {
            error!("Error: {}", e); 
            -1
        }
    }
}


fn rmain() -> Result<(), &'static str> {
    let my_cpu = cpu::current_cpu();

    let ready = Arc::new(Mutex::new(false));
    let ready2 = ready.clone();
    let ready3 = ready.clone();
    let wc = Arc::new(WaitCondition::new(move || *ready.lock() == true));
    let wc2 = wc.clone();


    let t1 = spawn::new_task_builder(wait_task, (wc, ready3))
        .name(String::from("wait_task"))
        .pin_on_core(my_cpu)
        .spawn()?;

    let t2 = spawn::new_task_builder(notify_task, (wc2, ready2))
        .name(String::from("notify_task"))
        .pin_on_core(my_cpu)
        .block()
        .spawn()?;
        
    let t3 = spawn::new_task_builder(
        |tref| {
            warn!("DeezNutz:  testing spurious wakeup on task {:?}", tref);
            tref.unblock().unwrap();
        }, 
        t1.clone(),
        )
        .name(String::from("deeznutz"))
        .pin_on_core(my_cpu)
        .block()
        .spawn()?;

        warn!("Finished spawning the 3 tasks");

    // give the wait task (t1) a chance to run before the notify task
    for _ in 0..100 { scheduler::schedule(); }
    t3.unblock().unwrap();
    
    for _ in 0..100 { scheduler::schedule(); }
    t2.unblock().unwrap();


    t1.join()?;
    t2.join()?;
    t3.join()?;
    warn!("Joined the 3 tasks");
    
    Ok(())
}


fn wait_task<WF: WaitConditionFn>((wc, ready): (Arc<WaitCondition<WF>>, Arc<Mutex<bool>>)) -> Result<(), &'static str> {
    warn!("  wait_task:  entered task. Calling wait()...");
    let retval = wc.wait();
    warn!("  wait_task:  wait() returned {:?}", retval);
    warn!("  wait_task:  after waiting, ready is {:?}", ready);
    retval.map_err(|_e| "wc.wait() error")
}


fn notify_task<WF: WaitConditionFn>((wc, ready): (Arc<WaitCondition<WF>>, Arc<Mutex<bool>>)) -> Result<bool, &'static str> {
    warn!("  notify_task:  entered task.");
    warn!("  notify_task:  setting ready to true, calling notify_one()...");
    *ready.lock() = true;
    let cond_sat = wc.condition_satisfied().ok_or("condition wasn't properly satisfied")?;
    // panic!("intentional panic in test_wait_queue2");
    let woken_up = cond_sat.notify_one();
    warn!("  notify_task:  notified a task? {}", woken_up);
    Ok(woken_up)
}
