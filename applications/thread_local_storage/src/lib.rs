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
pub static LOCAL_ZERO: u16 = 7;

#[thread_local]
pub static LOCAL_U8_1: u8 = 9;

#[thread_local]
pub static LOCAL_U16: u16 = 7;

#[thread_local]
pub static LOCAL_USIZE: usize = 4;

#[thread_local]
pub static LOCAL_U8_2: u8 = 9;

#[thread_local]
pub static MY_STRUCT: Cell<MyStruct> = Cell::new(MyStruct(0x12345678DEADBEEF));

#[thread_local]
pub static COMPLEX: Cell<Complex> = Cell::new(Complex::new());


#[derive(Debug)]
pub struct MyStruct(usize);
impl Drop for MyStruct {
    fn drop(&mut self) {
        warn!("DROPPING MyStruct({})", self.0);
    }
}

pub struct Complex {
    pub s: &'static str,
    pub m: u8,
    pub x: u32,
    pub l: usize,
    pub v: Vec<usize>,
}
impl Complex {
    const fn new() -> Complex {
        Complex { s: "hello there how", m: 8, x: 32, l: 0xDEAD, v: Vec::new() }
    }
}

pub fn test_tls() {
    debug!("Task {:?}: LOCAL_USIZE: {:?}", task::get_my_current_task(), &LOCAL_USIZE);
    debug!("Task {:?}: MY_STRUCT: {:?}", task::get_my_current_task(), MY_STRUCT.replace(MyStruct(0x99999999)));
}

pub fn main(_args: Vec<String>) -> isize {
    test_tls();
    0
}
