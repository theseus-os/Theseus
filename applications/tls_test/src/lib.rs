#![no_std]
// #![feature(plugin)]
// #![plugin(application_main_fn)]

extern crate alloc;
#[macro_use] extern crate log;
#[macro_use] extern crate terminal_print;
extern crate test_thread_local;
#[macro_use] extern crate thread_local_macro;

// use core::cell::RefCell;

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

    warn!("Accessing CONST_USIZE...");
    CONST_USIZE.with(|val| {
        warn!("CONST_USIZE has val {:X?}", val);
    });

    warn!("Accessing MY_STRUCT...");
    MY_STRUCT.with(|val| {
        warn!("MY_STRUCT has val {:X?}", val);
    });
}

#[derive(Debug)]
pub struct MyStruct(usize);
impl MyStruct {
    fn new(a: usize) -> MyStruct {
        warn!("MyStruct::new({:X?})", a);
        MyStruct(a)
    }
}
impl Drop for MyStruct {
    fn drop(&mut self) {
        warn!("DROPPING MyStruct({:X?})", self.0);
    }
}
