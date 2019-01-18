#![no_std]
#![feature(alloc)]

#[macro_use] extern crate log;
extern crate alloc;
extern crate spawn;

use spawn::KernelTaskBuilder;
use alloc::string::String;
use alloc::vec::Vec;

#[no_mangle]
pub fn main(args: Vec<String>) -> (){
    KernelTaskBuilder::new(test1 ,1)
        .name(String::from("test1"))
        .pin_on_core(1)
        .set_priority(30)
        .spawn().expect("failed to initiate task");

    KernelTaskBuilder::new(test2 ,2)
        .name(String::from("test2"))
        .pin_on_core(1)
        .set_priority(20)
        .spawn().expect("failed to initiate task");

    KernelTaskBuilder::new(test3 ,3)
        .name(String::from("test3"))
        .pin_on_core(1)
        .set_priority(10)
        .spawn().expect("failed to initiate task");
}

fn test1(_a: u32) -> u32 {
    let mut i = 1;
    loop {
       debug!("A {}", i);
       i = i + 1; 
    }
    _a
}

fn test2(_a: u32) -> u32 {
    let mut i = 1;
    loop {
       debug!("B {}", i);
       i = i + 1; 
    }
    _a
}

fn test3(_a: u32) -> u32 {
    let mut i = 1;
    loop {
       debug!("C {}", i);
       i = i + 1; 
    }
    _a
}