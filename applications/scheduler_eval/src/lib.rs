#![no_std]
#![feature(alloc)]

#[macro_use] extern crate log;
extern crate alloc;
extern crate spawn;
extern crate runqueue;

use spawn::KernelTaskBuilder;
use alloc::string::String;
use alloc::vec::Vec;


#[no_mangle]
pub fn main(args: Vec<String>) -> (){
    let taskref1 = KernelTaskBuilder::new(test1 ,1)
        .name(String::from("test1"))
        .pin_on_core(1)
        .set_priority(30)
        .spawn().expect("failed to initiate task");

    runqueue::assign_priority(&taskref1, 30).expect("failed to set priority for task 1");

    debug!("Completed Task 1");

    let taskref2 = KernelTaskBuilder::new(test2 ,2)
        .name(String::from("test2"))
        .pin_on_core(1)
        .set_priority(20)
        .spawn().expect("failed to initiate task");

    runqueue::assign_priority(&taskref2, 20).expect("failed to set priority for task 2");

    debug!("Completed Task 2");

    let taskref3 = KernelTaskBuilder::new(test3 ,3)
        .name(String::from("test3"))
        .pin_on_core(1)
        .set_priority(10)
        .spawn().expect("failed to initiate task");

    runqueue::assign_priority(&taskref3, 10).expect("failed to set priority for task 3");

    debug!("Completed Task 3");

    debug!("Completed Task all");
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