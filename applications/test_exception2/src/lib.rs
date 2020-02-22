#![no_std]


// extern crate alloc;
// #[macro_use] extern crate log;
extern crate serial_port;
// #[macro_use] extern crate terminal_print;
// extern crate task;


// use alloc::vec::Vec;
// use alloc::string::String;

#[derive(Debug)]
struct MyStruct2(pub usize);
impl MyStruct2 {
    #[inline(never)]
    pub fn new(val: usize) -> MyStruct2 {
        MyStruct2(val)
    }
}
impl Drop for MyStruct2 {
    #[inline(never)]
    fn drop(&mut self) {
        serial_port::write_test("*** DROPPING MYSTRUCT2 ***");
    }
}

// pub fn main(_args: Vec<String>) -> isize {
pub fn main(val: usize) {

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
        let _my_struct2 = MyStruct2::new(val);
        
        // cause page fault exception by dereferencing random memory value
        unsafe { *(0x5050DEADBEEF as *mut usize) = 0x5555_5555_5555; }
        let _res = serial_port::write_fmt(format_args!("{:?}", _my_struct2));
    }

    loop { }
    // 0
}
