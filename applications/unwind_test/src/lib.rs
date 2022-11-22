#![no_std]


extern crate alloc;
#[macro_use] extern crate log;
// #[macro_use] extern crate app_io;
extern crate task;
extern crate catch_unwind;


use alloc::vec::Vec;
use alloc::boxed::Box;
use alloc::string::String;


struct MyStruct(pub usize);
impl Drop for MyStruct {
    fn drop(&mut self) {
        warn!("\nDROPPING MYSTRUCT({})\n", self.0);
    }
}

#[inline(never)]
fn foo(cause_page_fault: bool) {
    let _res = task::set_kill_handler(Box::new(|kill_reason| {
        info!("unwind_test: caught kill action at {}", kill_reason);
    }));
    
    let _my_struct = MyStruct(10);
    if cause_page_fault {
        // dereference random memory value
        unsafe { *(0x5050DEADBEEF as *mut usize) = 0x5555_5555_5555; }
    } 
    else {
        panic!("intentional panic in unwind_test::foo()");
    }
}


pub fn main(args: Vec<String>) -> isize {
    let _my_struct = MyStruct(5);

    match args.get(0).map(|s| &**s) {
        // cause a page fault to test unwinding through a machine exception
        Some("-e") => foo(true),
        // test catch_unwind and then resume_unwind
        Some("-c") => catch_resume_unwind(),
        _ => foo(false),
    };

    error!("Test failure: unwind_test::main should not return!");

    0
}

#[inline(never)]
fn catch_resume_unwind() {
    let _my_struct6 = MyStruct(6);

    let res = catch_unwind::catch_unwind_with_arg(fn_to_catch, MyStruct(22));
    warn!("CAUGHT UNWINDING ACTION, as expected.");
    let _my_struct7 = MyStruct(7);
    if let Err(e) = res {
        let _my_struct8 = MyStruct(8);
        catch_unwind::resume_unwind(e);
    }

    error!("Test failure: catch_resume_unwind should not return!");
}

#[inline(never)]
fn fn_to_catch(_s: MyStruct) {
    let _my_struct9 = MyStruct(9);

    panic!("intentional panic in unwind_test::fn_to_catch()")
}
