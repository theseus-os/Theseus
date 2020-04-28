#![no_std]


extern crate alloc;
#[macro_use] extern crate log;
// #[macro_use] extern crate terminal_print;
extern crate task;


use alloc::vec::Vec;
use alloc::boxed::Box;
use alloc::string::String;


struct MyStruct(pub usize);
impl Drop for MyStruct {
    fn drop(&mut self) {
        warn!("DROPPING MYSTRUCT({})", self.0);
    }
}

#[inline(never)]
fn foo(cause_page_fault: bool) {
    let _res = task::set_my_kill_handler(Box::new(|kill_reason| {
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


pub fn main(_args: Vec<String>) -> isize {

    // // dump some info about the this loaded app crate
    // {
    //     let curr_task = task::get_my_current_task().unwrap();
    //     let t = curr_task.lock();
    //     let app_crate = t.app_crate.as_ref().unwrap();
    //     let krate = app_crate.lock_as_ref();
    //     trace!("============== Crate {} =================", krate.crate_name);
    //     for s in krate.sections.values() {
    //         trace!("   {:?}", &*s.lock());
    //     }
    // }

    let _my_struct = MyStruct(5);

    let cause_page_fault = match _args.get(0).map(|s| &**s) {
        Some("-e") => true,
        _ => false,
    };

    foo(cause_page_fault);
    0
}
