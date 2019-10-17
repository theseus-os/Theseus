#![no_std]
// #![feature(plugin)]
// #![plugin(application_main_fn)]


extern crate alloc;
// #[macro_use] extern crate log;
#[macro_use] extern crate terminal_print;
extern crate libc;

use alloc::vec::Vec;
use alloc::string::String;
use libc::*;
use libc::types::*;


#[no_mangle]
pub fn main(_args: Vec<String>) -> isize {
    println!("testing libc!!!");
    let size = 64;

    // malloc test
    unsafe {
        let a = mman::malloc(size) as *mut u8; 
        *a = 0;
        *a.offset(size as isize - 1) = size as u8;

        mman::free(a as *mut c_void);
        // once added back to the heap, the size of this area is stored in the first byte
        assert_eq!(*a, size as u8);
    }

    //mmap test 
    unsafe {
        let ptr = mman::mmap(0 as *mut c_void, 4096, 0, mman::MAP_ANON, 0, 0);
        println!("mapped pages allocated start at: {:#X}", ptr as usize);
        mman::munmap(ptr, 4096);
        // mman::munmap(ptr, 4096);
    }

    //file test
    unsafe {
        let name = c_str::CStr::from_bytes_with_nul(b"hello\0").unwrap(); 
        let fd = fs::open(name, fcntl::O_CREAT ,0);
        // let a = fs::close(fd);
    }
     
    0
}
