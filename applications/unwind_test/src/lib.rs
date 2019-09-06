#![no_std]


extern crate alloc;
#[macro_use] extern crate log;
// #[macro_use] extern crate terminal_print;
extern crate task;


use alloc::vec::Vec;
use alloc::string::String;


struct MyStruct(pub usize);
impl Drop for MyStruct {
    fn drop(&mut self) {
        warn!("DROPPING MYSTRUCT({})", self.0);
    }
}

#[inline(never)]
fn foo() {
    let _my_struct = MyStruct(10);
    panic!("intentional panic in unwind_test::foo()");
}


#[no_mangle]
pub fn main(_args: Vec<String>) -> isize {
    let _my_struct = MyStruct(5);
    foo();
    0
}
