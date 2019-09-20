#![no_std]

#[macro_use] extern crate log;
extern crate alloc;

use alloc::alloc::Layout;
use alloc::alloc::alloc;

type off_t = i64;
type size_t = usize;


/// Creates a layout for the memory required and returns a pointer from the heap 
/// void *malloc(size_t size);
pub extern fn rmalloc (size: size_t) -> *mut u8 {
    // we set the alignment to 1 byte, as there is noe pre-requisite in the malloc function
    const alignment: usize = 1;

    let layout = Layout::from_size_align(size, alignment).unwrap();

    let mem = unsafe{ alloc(layout) };

    unsafe {
        // remove below just for testing
        *mem = 3;
        *mem.offset(1) = 4;

        // remove above 
    }

    
    return mem;
}

// void *mmap(void *addr, size_t length, int prot, int flags, int fd, off_t offset);
// fn mmap(addr: *mut u8, length: size_t, prot: i32, flags: i32, fd: i32, offset: off_t) -> *mut u8 {

// }

// fn munmap
