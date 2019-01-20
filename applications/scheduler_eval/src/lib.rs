#![no_std]
#![feature(alloc)]

#[macro_use] extern crate log;
extern crate alloc;
extern crate spawn;
extern crate scheduler;

use spawn::KernelTaskBuilder;
use alloc::string::String;
use alloc::vec::Vec;


#[no_mangle]
pub fn main(args: Vec<String>) -> (){
    let taskref1 = KernelTaskBuilder::new(test1 ,1)
        .name(String::from("test1"))
        .pin_on_core(1)
        .spawn().expect("failed to initiate task");

    scheduler::set_priority(&taskref1, 30).expect("failed to set priority for task 1");

    debug!("Spawned Task 1");

    let taskref2 = KernelTaskBuilder::new(test2 ,2)
        .name(String::from("test2"))
        .pin_on_core(1)
        .spawn().expect("failed to initiate task");

    scheduler::set_priority(&taskref2, 20).expect("failed to set priority for task 2");

    debug!("Spawned Task 2");

    let taskref3 = KernelTaskBuilder::new(test3 ,3)
        .name(String::from("test3"))
        .pin_on_core(1)
        .spawn().expect("failed to initiate task");

    scheduler::set_priority(&taskref3, 10).expect("failed to set priority for task 3");

    debug!("Spawned Task 3");

    debug!("Spawned all tasks");

    let priority1 = scheduler::get_priority(&taskref1);
    let priority2 = scheduler::get_priority(&taskref2);
    let priority3 = scheduler::get_priority(&taskref3);

    #[cfg(priority_scheduler)]
    {
        assert_eq!(priority1,Some(30));
        assert_eq!(priority2,Some(20));
        assert_eq!(priority3,Some(10));
    }

    
}

fn test1(_a: u32) -> u32 {
    let mut i = 1;
    loop{
    //for i in 0..10 {
       debug!("A {}", i);
       i = i + 1; 
    }
    _a
}

fn test2(_a: u32) -> u32 {
    let mut i = 1;
    loop{
    //for i in 0..10 {
       debug!("B {}", i);
       i = i + 1; 
    }
    _a
}

fn test3(_a: u32) -> u32 {
    let mut i = 1;
    loop{
    //for i in 1..10 {
       debug!("C {}", i);
       i = i + 1; 
    }
    _a
}