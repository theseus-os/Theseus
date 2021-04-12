#![no_std]


// extern crate alloc;
// #[macro_use] extern crate log;
extern crate logger;
// #[macro_use] extern crate terminal_print;
// extern crate task;


// use alloc::vec::Vec;
// use alloc::string::String;


struct MyStruct(pub usize);
impl Drop for MyStruct {
    #[inline(never)]
    fn drop(&mut self) {
        logger::write_str("*** DROPPING MYSTRUCT ***").unwrap();
    }
}

// pub fn main(_args: Vec<String>) -> isize {
pub unsafe fn main() {

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

    {
        let _my_struct = MyStruct(5);
        
        // cause page fault exception by dereferencing random memory value
        *(0x5050DEADBEEF as *mut usize) = 0x5555_5555_5555;
    }

    loop { }
    // 0
}
