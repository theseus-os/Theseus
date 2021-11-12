//! Offers thread-local storage (TLS) in Theseus, or rather, task-local storage.
//! 
//! This crate offers fully-fledged support for fast access to and dynamic allocation of
//! arbitrary objects that will exist on a per-task (per-"thread") basis.
//! It integrates with Rust's `#[thread_local]` attribute to offer features similar to 
//! that of the Rust standard library, e.g., the `thread_local!()` macro,
//! in a more ergonomic way more friendly to struct composition.

#![no_std]
#![feature(thread_local)]

extern crate alloc;
#[macro_use] extern crate log;
extern crate spawn;
extern crate task; 

use core::cell::Cell;
use alloc::{string::String, vec::Vec};

#[thread_local]
pub static LOCAL_ZERO: Cell<u16> = Cell::new(0);

#[thread_local]
pub static LOCAL_U8_1: Cell<u8> = Cell::new(91);

#[thread_local]
pub static LOCAL_U16: Cell<u16> = Cell::new(71);

#[thread_local]
pub static LOCAL_USIZE: Cell<usize> = Cell::new(4);

#[thread_local]
pub static LOCAL_U8_2: Cell<u8> = Cell::new(9);

#[thread_local]
pub static MY_STRUCT: Cell<MyStruct> = Cell::new(MyStruct(0x12345678DEADBEEF));

#[thread_local]
// pub static COMPLEX: Complex = Complex::new();
pub static COMPLEX: Cell<Complex> = Cell::new(Complex::new());


#[derive(Debug, Clone, Copy)]
pub struct MyStruct(usize);
// impl Drop for MyStruct {
//     fn drop(&mut self) {
//         warn!("DROPPING MyStruct({})", self.0);
//     }
// }

#[derive(Debug, Clone, Copy)]
pub struct Complex {
    pub s: &'static str,
    pub m: u8,
    pub x: u32,
    pub l: usize,
}
impl Complex {
    const fn new() -> Complex {
        Complex { s: "hello there now", m: 0x8, x: 0x20, l: 0xDEAD }
    }
}

pub fn test_tls(_x: usize) {
    let curr_task = task::get_my_current_task().unwrap();
    let local_zero = LOCAL_ZERO.get();
    debug!("Task {:?}, LOCAL_ZERO: {:#X?}", curr_task, local_zero);

    let local_u8_1 = LOCAL_U8_1.get();
    debug!("Task {:?}, LOCAL_U8_1: {:#X?}", curr_task, local_u8_1);

    let local_u16 = LOCAL_U16.get();
    debug!("Task {:?}, LOCAL_U16: {:#X?}", curr_task, local_u16);

    let local_usize = LOCAL_USIZE.get();
    debug!("Task {:?}, LOCAL_USIZE: {:#X?}", curr_task, local_usize);

    let local_u8_2 = LOCAL_U8_2.get();
    debug!("Task {:?}, LOCAL_U8_2: {:#X?}", curr_task, local_u8_2);

    debug!("Task {:?}, COMPLEX: {:X?}", curr_task, COMPLEX);
    debug!("Task {:?}, COMPLEX: {:X?}", curr_task, COMPLEX.replace(Complex {
        s: "this is the new string value",
        m: 0x5D,
        x: 0xEB,
        l: 0xBEEF,
    }));
    debug!("Task {:?}, after replacing COMPLEX, its new value is: {:X?}", curr_task, COMPLEX.get());

    debug!("Task {:?}, MY_STRUCT: {:X?}", curr_task, MY_STRUCT);
    debug!("Task {:?}: MY_STRUCT: {:X?}", curr_task, MY_STRUCT.replace(MyStruct(0x99999999)));
    debug!("Task {:?}: after setting MY_STRUCT to 0x99999999, its new value is {:X?}", curr_task, MY_STRUCT.get());
}

pub fn main(_args: Vec<String>) -> isize {
    test_tls(5);
    0
}
