#![no_std]

#[macro_use] extern crate log;
extern crate alloc;
extern crate spawn;
extern crate scheduler;
extern crate task;

use alloc::string::String;
use alloc::vec::Vec;


pub fn main(_args: Vec<String>) -> isize {
    let taskref1 = spawn::new_task_builder(worker, ())
        .name(String::from("test1"))
        .pin_on_core(1)
        .spawn().expect("failed to initiate task");

    if let Err(e) = scheduler::set_priority(&taskref1, 30) {
        error!("scheduler_eval(): Could not set priority to taskref1: {}", e);
    }

    debug!("Spawned Task 1");

    let taskref2 = spawn::new_task_builder(worker, ())
        .name(String::from("test2"))
        .pin_on_core(1)
        .spawn().expect("failed to initiate task");

    if let Err(e) = scheduler::set_priority(&taskref2, 20) {
        error!("scheduler_eval(): Could not set priority to taskref2: {}", e);
    }

    debug!("Spawned Task 2");

    let taskref3 = spawn::new_task_builder(worker, ())
        .name(String::from("test3"))
        .pin_on_core(1)
        .spawn().expect("failed to initiate task");

    if let Err(e) = scheduler::set_priority(&taskref3, 10) {
        error!("scheduler_eval(): Could not set priority to taskref3: {}", e);
    }

    debug!("Spawned Task 3");

    debug!("Spawned all tasks");

    let _priority1 = scheduler::get_priority(&taskref1);
    let _priority2 = scheduler::get_priority(&taskref2);
    let _priority3 = scheduler::get_priority(&taskref3);

    #[cfg(priority_scheduler)]
    {
        assert_eq!(_priority1,Some(30));
        assert_eq!(_priority2,Some(20));
        assert_eq!(_priority3,Some(10));
    }

    taskref1.join().expect("Task 1 join failed");
    taskref2.join().expect("Task 2 join failed");
    taskref3.join().expect("Task 3 join failed");

    0
}

fn worker(_: ()) {
    for i in 0..1000 {
       debug!("Task_ID : {} , Instance : {}", task::get_my_current_task_id(), i);
       scheduler::schedule();
    }
}
