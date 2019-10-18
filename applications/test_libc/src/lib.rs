#![no_std]


extern crate alloc;
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
        let a = stdlib::malloc(size) as *mut u8; 
        *a = 0;
        *a.offset(size as isize - 1) = size as u8;

        stdlib::free(a as *mut c_void);
    }

    //mmap test 
    unsafe {
        let ptr = mman::mmap(0 as *mut c_void, 4096, 0, mman::MAP_ANON, 0, 0);
        println!("mapped pages allocated start at: {:#X}", ptr as usize);
        mman::munmap(ptr, 4096);
        // mman::munmap(ptr, 4096);
    }

    //file open close test
    unsafe {
        let name = c_str::CStr::from_bytes_with_nul(b"hello\0").unwrap(); 
        let fd1 = fs::open(name, fcntl::O_CREAT ,0);
        println!("recieved file descriptor : {}", fd1);
        let fd2 = fs::open(name, fcntl::O_CREAT ,0);
        println!("recieved file descriptor : {}", fd2);
        
        let fd3 = fs::open(name, fcntl::O_CREAT ,0);
        println!("recieved file descriptor : {}", fd3);
        let a = fs::close(fd3);
        let a = fs::close(fd1);
        let a = fs::close(fd2);
    }

    // file read/write test
    unsafe {
        let name = c_str::CStr::from_bytes_with_nul(b"rwtest\0").unwrap(); 
        let fdw = fs::open(name, fcntl::O_CREAT, 0);
        let message: [u8; 5] = [5,7,2,34,79];
        fs::write(fdw, message.as_ptr() as *const c_void, 5);
        
        let fdr = fs::open(name, fcntl::O_CREAT, 0);    
        let mut message2: [u8; 5] = [0;5];
        fs::read(fdr, message2.as_mut_ptr() as *mut c_void, 5);
        println!("{:?}", message2);

        let a = fs::close(fdw);
        let b = fs::close(fdr);
        println!("{}, {}", a, b);

    }
     
    0
}
