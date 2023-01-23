#![no_std]
// #![feature(plugin)]
// #![plugin(application_main_fn)]
//! Tests the read/write capability of the memory-backed file (memfs::MemFile)

#[macro_use] extern crate alloc;
// #[macro_use] extern crate log;
#[macro_use] extern crate app_io;
extern crate memfs;
extern crate root;
extern crate memory;

use memfs::MemFile;
use alloc::vec::Vec;
use alloc::string::{String, ToString};
use core::str;


fn test_filerw() -> Result<(), &'static str> {
    let parent = root::get_root();
    let testfile = MemFile::create("testfile".to_string(), &parent)?;

    // test that we can write to an empty file
    testfile.lock().write_at("test from hello".as_bytes(),0)?;
    let file_size = testfile.lock().len();
    let mut string_slice_as_bytes = vec![0; file_size];
    testfile.lock().read_at(&mut string_slice_as_bytes, 5)?;
    println!("first test file string is {}", str::from_utf8(&mut string_slice_as_bytes).unwrap());
    println!("size of test file should be 15, actual is {}", testfile.lock().len());

    // test that we can overwrite overlapping existing content
    testfile.lock().write_at("OVERWRITE".as_bytes(), 5)?;
    let mut string_slice_as_bytes2 = vec![0; file_size];
    testfile.lock().read_at(&mut string_slice_as_bytes2, 0)?;
    println!("second test file string is: {}", str::from_utf8(&mut string_slice_as_bytes2).unwrap());
    println!("size of testfile should be 15, actual is {}", testfile.lock().len());
    println!("second test successful: overwrote file contents at an offset");

    // test that we can simply overwrite all file contents
    testfile.lock().write_at("OVERWRITE ALL CONTENTS".as_bytes(), 0)?;
    let mut full_overwrite_bytes = vec![0; "OVERWRITE ALL CONTENTS".as_bytes().len()];
    testfile.lock().read_at(&mut full_overwrite_bytes, 0)?;
    println!("third test file string is: {}", str::from_utf8(&mut full_overwrite_bytes).unwrap());
    println!("size of testfile should be {}, actual is {}", 22, testfile.lock().len());
    println!("third test successful: fully overwrote existing file content");

    // testing reallocation when file contents exceeds existing MappedPages capacity
    testfile.lock().write_at("hello way down here".as_bytes(), 4094)?;
    let mut reallocate2 = vec![0; "hello way down here".as_bytes().len()];
    testfile.lock().read_at(&mut reallocate2, 4094)?;
    println!("fourth read is: {}", str::from_utf8(&mut reallocate2).unwrap());
    println!("size of testfile should be {}, actual is {}", 4094 + 19, testfile.lock().len());
    println!("fourth test successful: expanded file to two mapped pages via reallocation");


    // another test for reallocation, but bigger this time
    testfile.lock().write_at("reallocated to four mapped pages".as_bytes(), 4096 * 4 + 5)?;
    let mut reallocate4 = vec![0; "reallocated to four mapped pages".as_bytes().len()];
    testfile.lock().read_at(&mut reallocate4, 4096 * 4 + 5)?;
    println!("fifth read is: {}", str::from_utf8(&mut reallocate4).unwrap());
    println!("size of testfile should be {}, actual is {}", 4096 * 4 + 5 + 32, testfile.lock().len());
    println!("fifth test successful: expanded file to four mapped pages via reallocation");

    // testing that we cannot write to a MemFile with MappedPages that aren't Writeable
    // first we obtain non-writable mapped pages
        // Obtain the active kernel page table
    let kernel_mmi_ref = memory::get_kernel_mmi_ref().ok_or("KERNEL_MMI was not yet initialized!")?;
    // Allocate and map the least number of pages we need to store the information contained in the buffer
    // we'll allocate the buffer length plus the offset because that's guranteed to be the most bytes we
    // need (because it entered this conditional statement)
    let pages = memory::allocate_pages_by_bytes(1).ok_or("could not allocate pages")?;
    let mapped_pages = kernel_mmi_ref.lock().page_table.map_allocated_pages(pages, memory::PteFlags::new())?;

    let non_writable_file = MemFile::from_mapped_pages(mapped_pages, "non-writable testfile".to_string(), 1, &parent)?;
    match non_writable_file.lock().write_at(&mut string_slice_as_bytes, 0) {
        Ok(_) => println!("should not have been able to write"),
        Err(_err) => println!("sixth test successful\nsuccessfully failed to write to nonwritable mapped pages")
    }



    // tests that the read function works when we pass an oversized buffer with an offset that exceeds the end of the file
    let testfile2 = MemFile::create("testfile2".to_string(), &parent)?;
    testfile2.lock().write_at("test from hello".as_bytes(),0)?;
    let mut oversize_buffer = vec![0; 4 * file_size];
    let test1_bytesread = testfile2.lock().read_at(&mut oversize_buffer, 0)?;
    println!("seventh test (part 1) should have read 15 bytes, actually read {} bytes", test1_bytesread);
    println!("seventh test read output (part 1) should be 'test from hello', actually is {} ", str::from_utf8(&mut oversize_buffer).unwrap());
    let mut oversize_buffer2 = vec![0; 4* file_size];
    let test2_bytesread = testfile2.lock().read_at(&mut oversize_buffer2, 5)?;
    println!("seventh test (part 2) should have read 10 bytes, actually read {} bytes", test2_bytesread);
    println!("seventh test read output (part 1) should be 'from hello', actually is {} ", str::from_utf8(&mut oversize_buffer2).unwrap());    
    println!("seventh test successful: read with oversized buffers works");


    Ok(())
}

pub fn main(_args: Vec<String>) -> isize {

    match test_filerw() {
        Ok(()) => { },
        Err(err) => println!("{}", err)
    }

    0
}
