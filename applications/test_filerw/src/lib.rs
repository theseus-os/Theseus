#![no_std]
#![feature(alloc)]
// #![feature(plugin)]
// #![plugin(application_main_fn)]
//! Tests the read/write capability of the memory-backed file (memfs::MemFile)

#[macro_use] extern crate alloc;
#[macro_use] extern crate log;
#[macro_use] extern crate terminal_print;
extern crate memfs;
extern crate root;
extern crate memory;

use memfs::MemFile;
use alloc::vec::Vec;
use alloc::string::{String, ToString};
use core::str;
use core::ops::DerefMut;
use memory::{MappedPages, FRAME_ALLOCATOR, EntryFlags};
use alloc::sync::Arc;



fn test_filerw() -> Result<(), &'static str> {
    let parent = root::get_root();
    let testfile = MemFile::new("testfile".to_string(), &parent)?;

    // test that we can write to an empty file
    testfile.lock().write("test from hello".as_bytes(),0)?;
    let file_size = testfile.lock().size();
    let mut string_slice_as_bytes = vec![0; file_size];
    testfile.lock().read(&mut string_slice_as_bytes, 5)?;
    println!("first test file string is {}", str::from_utf8(&mut string_slice_as_bytes).unwrap());
    println!("size of test file should be 15, actual is {}", testfile.lock().size());


    // tests that the read function works when we pass an oversized buffer with an offset that exceeds the end of the file
    let mut oversize_buffer = vec![0; 4 * file_size];
    let test1_bytesread = testfile.lock().read(&mut oversize_buffer, 5)?;
    println!("first test (part 2) should have read 10 bytes, actually read {} bytes", test1_bytesread);
    println!("first test read output should be 'from hello', actually is '{}'", str::from_utf8(&mut oversize_buffer).unwrap());
    println!("first test successful: wrote to empty file");

    // test that we can overwrite overlapping existing content
    testfile.lock().write("OVERWRITE".as_bytes(), 5)?;
    let mut string_slice_as_bytes2 = vec![0; file_size];
    testfile.lock().read(&mut string_slice_as_bytes2, 0)?;
    println!("second test file string is: {}", str::from_utf8(&mut string_slice_as_bytes2).unwrap());
    println!("size of testfile should be 15, actual is {}", testfile.lock().size());
    println!("second test successful: overwrote file contents at an offset");

    // test that we can simply overwrite all file contents
    testfile.lock().write("OVERWRITE ALL CONTENTS".as_bytes(), 0)?;
    let mut full_overwrite_bytes = vec![0; file_size];
    testfile.lock().read(&mut full_overwrite_bytes, 0)?;
    println!("third test file string is: {}", str::from_utf8(&mut full_overwrite_bytes).unwrap());
    println!("size of testfilel should be {}, actual is {}", 22, testfile.lock().size());
    println!("third test successful: fully overwrote existing file content");

    // testing reallocation when file contents exceeds existing MappedPages capacity
    testfile.lock().write("hello way down here".as_bytes(), 4094)?;
    let mut reallocate2 = vec![0; "hello way down here".as_bytes().len()];
    testfile.lock().read(&mut reallocate2, 4094)?;
    println!("fourth read is: {}", str::from_utf8(&mut reallocate2).unwrap());
    println!("size of testfile should be {}, actual is {}", 4094 + 19, testfile.lock().size());
    println!("fourth test successful: expanded file to two mapped pages via reallocation");


    // another test for reallocation, but bigger this time
    testfile.lock().write("reallocated to four mapped pages".as_bytes(), 4096 * 4 + 5)?;
    let mut reallocate4 = vec![0; "reallocated to four mapped pages".as_bytes().len()];
    testfile.lock().read(&mut reallocate4, 4096 * 4 + 5)?;
    println!("fifth read is: {}", str::from_utf8(&mut reallocate4).unwrap());
    println!("size of testfile should be {}, actual is {}", 4096 * 4 + 5 + 32, testfile.lock().size());
    println!("fifth test successful: expanded file to four mapped pages via reallocation");

    // testing that we cannot write to a MemFile with MappedPages that aren't Writeable
    // first we obtain non-writable mapped pages
        // Obtain the active kernel page table
    let kernel_mmi_ref = memory::get_kernel_mmi_ref().ok_or("KERNEL_MMI was not yet initialized!")?;
    let mut mapped_pages = MappedPages::empty(); // we're going to overwrite this in the conditional
    if let memory::PageTable::Active(ref mut active_table) = kernel_mmi_ref.lock().page_table {
        let mut allocator = try!(FRAME_ALLOCATOR.try().ok_or("Couldn't get Frame Allocator"));
        // Allocate and map the least number of pages we need to store the information contained in the buffer
        // we'll allocate the buffer length plus the offset because that's guranteed to be the most bytes we
        // need (because it entered this conditional statement)
        let pages = memory::allocate_pages_by_bytes(1).ok_or("could not allocate pages")?;
        // the default flag is that the MappedPages are not writable
        mapped_pages = active_table.map_allocated_pages(pages, Default::default() , allocator.lock().deref_mut())?;            
    }

    let non_writable_file = MemFile::from_mapped_pages(mapped_pages, "non-writable testfile".to_string(), 1, &parent)?;
    match non_writable_file.lock().write(&mut string_slice_as_bytes, 0) {
        Ok(_) => println!("should not have been able to write"),
        Err(err) => println!("sixth test successful\nsuccessfully failed to write to nonwritable mapped pages")
    }

    Ok(())
}

#[no_mangle]
pub fn main(_args: Vec<String>) -> isize {

    match test_filerw() {
        Ok(()) => { },
        Err(err) => println!("{}", err)
    }

    0
}
