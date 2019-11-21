#![no_std]

#[macro_use] extern crate log;
extern crate alloc;
extern crate spawn;
extern crate scheduler;
extern crate task;

use spawn::KernelTaskBuilder;
use alloc::string::String;
use alloc::vec::Vec;


#[no_mangle]
pub fn main(_args: Vec<String>) -> (){
    let taskref1 = KernelTaskBuilder::new(test1 ,1)
        .name(String::from("test1"))
        .pin_on_core(3)
        .spawn().expect("failed to initiate task");

    if let Err(e) = scheduler::set_priority(&taskref1, 30) {
        error!("scheduler_eval(): Could not set priority to taskref1: {}", e);
    }

    debug!("Spawned Task 1");

    let taskref2 = KernelTaskBuilder::new(test2 ,2)
        .name(String::from("test2"))
        .pin_on_core(3)
        .spawn().expect("failed to initiate task");

    if let Err(e) = scheduler::set_priority(&taskref2, 20) {
        error!("scheduler_eval(): Could not set priority to taskref2: {}", e);
    }

    debug!("Spawned Task 2");

    let taskref3 = KernelTaskBuilder::new(test3 ,3)
        .name(String::from("test3"))
        .pin_on_core(3)
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
}

fn test1(_a: u32) -> u32 {
    //let mut i = 1;
    //loop{
    for i in 0..1000 {
       let task_id = match task::get_my_current_task_id() {
            Some(task_id) => {task_id},
            None => 0
       };
       debug!("Task_ID : {} , Instance : {}", task_id, i);
       scheduler::schedule();
       //i = i + 1; 
    }
    _a
}

fn test2(_a: u32) -> u32 {
    //let mut i = 1;
    //loop{
    for i in 0..1000 {
       let task_id = match task::get_my_current_task_id() {
            Some(task_id) => {task_id},
            None => 0
       };
       debug!("Task_ID : {} , Instance : {}", task_id, i);
       scheduler::schedule();
       //i = i + 1; 
    }
    _a
}

fn test3(_a: u32) -> u32 {
    //let mut i = 1;
    //loop{
    for i in 0..1000 {
       let task_id = match task::get_my_current_task_id() {
            Some(task_id) => {task_id},
            None => 0
       };
       debug!("Task_ID : {} , Instance : {}", task_id, i);
       scheduler::schedule();
       //i = i + 1; 
    }
    _a
}