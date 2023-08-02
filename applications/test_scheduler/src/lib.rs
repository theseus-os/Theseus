#![no_std]

#[macro_use] extern crate log;
extern crate alloc;
extern crate spawn;
extern crate scheduler;
extern crate task;
extern crate cpu;

use core::convert::TryFrom;

use alloc::string::String;
use alloc::vec::Vec;
use cpu::CpuId;


pub fn main(_args: Vec<String>) -> isize {
    let cpu_1 = CpuId::try_from(1).expect("CPU ID 1 did not exist");

    let taskref1 = spawn::new_task_builder(worker, ())
        .name(String::from("test1"))
        .pin_on_cpu(cpu_1)
        .spawn().expect("failed to initiate task");

    if let Err(e) = taskref1.set_priority(30) {
        error!("test_scheduler(): Could not set priority to taskref1: {}", e);
    }

    debug!("Spawned Task 1");

    let taskref2 = spawn::new_task_builder(worker, ())
        .name(String::from("test2"))
        .pin_on_cpu(cpu_1)
        .spawn().expect("failed to initiate task");

    if let Err(e) = taskref2.set_priority(20) {
        error!("test_scheduler(): Could not set priority to taskref2: {}", e);
    }

    debug!("Spawned Task 2");

    let taskref3 = spawn::new_task_builder(worker, ())
        .name(String::from("test3"))
        .pin_on_cpu(cpu_1)
        .spawn().expect("failed to initiate task");

    if let Err(e) = taskref3.set_priority(10) {
        error!("test_scheduler(): Could not set priority to taskref3: {}", e);
    }

    debug!("Spawned Task 3");

    debug!("Spawned all tasks");

    let _priority1 = taskref1.priority();
    let _priority2 = taskref2.priority();
    let _priority3 = taskref3.priority();

    #[cfg(epoch_scheduler)]
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
       task::schedule();
    }
}
