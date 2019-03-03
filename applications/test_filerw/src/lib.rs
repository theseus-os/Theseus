#![no_std]
#![feature(alloc)]
// #![feature(plugin)]
// #![plugin(application_main_fn)]


#[macro_use] extern crate alloc;
#[macro_use] extern crate log;
#[macro_use] extern crate terminal_print;
extern crate memfs;
extern crate root;

use memfs::MemFile;
use alloc::vec::Vec;
use alloc::string::{String, ToString};
use core::str;


#[no_mangle]
pub fn main(_args: Vec<String>) -> isize {

    let parent = root::get_root();
    let testfile = MemFile::new("testfile".to_string(), &parent).unwrap();

    // test that we can write to an empty file
    testfile.lock().write("test from hello".as_bytes(),0).unwrap();
    let file_size = testfile.lock().size();
    let mut string_slice_as_bytes = vec![0; file_size];
    testfile.lock().read(&mut string_slice_as_bytes, 0).unwrap();
    debug!("first test file string is: {}", str::from_utf8(&mut string_slice_as_bytes).unwrap());
    println!("first test file string is {}", str::from_utf8(&mut string_slice_as_bytes).unwrap());

    // test that we can overwrite overlapping existing content
    testfile.lock().write("OVERWRITE".as_bytes(), 5).unwrap();
    let mut string_slice_as_bytes2 = vec![0; file_size];
    testfile.lock().read(&mut string_slice_as_bytes2, 0).unwrap();
    debug!("second test file string is: {}", str::from_utf8(&mut string_slice_as_bytes2).unwrap());
    println!("second test file string is: {}", str::from_utf8(&mut string_slice_as_bytes2).unwrap());

    // testing reallocation when file contents exceeds existing MappedPages capacity
    testfile.lock().write("hello way down here".as_bytes(), 4094).unwrap();
    let mut string_slice_as_bytes3 = vec![0; "hello way down here".as_bytes().len()];
    testfile.lock().read(&mut string_slice_as_bytes3, 4094).unwrap();
    debug!("third read is: {}", str::from_utf8(&mut string_slice_as_bytes3).unwrap());
    println!("third read is: {}", str::from_utf8(&mut string_slice_as_bytes3).unwrap());
    0
}
