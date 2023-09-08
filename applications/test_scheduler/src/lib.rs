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

    if !scheduler::set_priority(&taskref1, 30) {
        error!("scheduler_eval(): Could not set priority to taskref1");
    }

    debug!("Spawned Task 1");

    let taskref2 = spawn::new_task_builder(worker, ())
        .name(String::from("test2"))
        .pin_on_cpu(cpu_1)
        .spawn().expect("failed to initiate task");

    if !scheduler::set_priority(&taskref2, 20) {
        error!("scheduler_eval(): Could not set priority to taskref2");
    }

    debug!("Spawned Task 2");

    let taskref3 = spawn::new_task_builder(worker, ())
        .name(String::from("test3"))
        .pin_on_cpu(cpu_1)
        .spawn().expect("failed to initiate task");

    if !scheduler::set_priority(&taskref3, 10) {
        error!("scheduler_eval(): Could not set priority to taskref3");
    }

    debug!("Spawned Task 3");

    debug!("Spawned all tasks");

    let _priority1 = scheduler::priority(&taskref1);
    let _priority2 = scheduler::priority(&taskref2);
    let _priority3 = scheduler::priority(&taskref3);

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
       scheduler::schedule();
    }
}
