//! Basic tests for thread-local storage (TLS) atop Theseus.

#![no_std]
// #![feature(plugin)]
// #![plugin(application_main_fn)]

extern crate alloc;
#[macro_use] extern crate log;
#[macro_use] extern crate app_io;
extern crate test_thread_local;
#[macro_use] extern crate thread_local_macro;

use alloc::vec::Vec;
use alloc::string::String;


pub fn main(args: Vec<String>) -> isize {

    match args.first() {
        Some(first) if first == "macro" => {
            test_macro();
            return 0;
        }
        _ => { }
    }

    println!("Invoking test_thread_local::test_tls()...");
    test_thread_local::test_tls(10);
    println!("Finished invoking test_thread_local::test_tls().");

    0
}


/// Tests the `thread_local!()` macro and its destructors at task exit.
fn test_macro() {
    thread_local! {
        static CONST_USIZE: usize = 0x1234;
        static MY_STRUCT: MyStruct = MyStruct::new(0x6565);
    }

    debug!("Accessing CONST_USIZE...");
    CONST_USIZE.with(|val| {
        debug!("CONST_USIZE has val {:X?}", val);
    });

    debug!("Accessing MY_STRUCT...");
    MY_STRUCT.with(|val| {
        debug!("MY_STRUCT has val {:X?}", val);
    });
}

#[derive(Debug)]
pub struct MyStruct(usize);
impl MyStruct {
    fn new(a: usize) -> MyStruct {
        debug!("MyStruct::new({:X?})", a);
        MyStruct(a)
    }
}
impl Drop for MyStruct {
    fn drop(&mut self) {
        debug!("DROPPING MyStruct({:X?})", self.0);
    }
}
