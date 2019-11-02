//! stdlib implementation for Theseus, following http://pubs.opengroup.org/onlinepubs/7908799/xsh/stdlib.h.html

use core::{convert::TryFrom, intrinsics, iter, mem, ptr, slice};
use rand::{
    distributions::{Alphanumeric, Distribution, Uniform},
    prng::XorShiftRng,
    rngs::JitterRng,
    Rng, SeedableRng,
};
use alloc::alloc::{Layout, alloc, dealloc};
use spin::Mutex;
use hashbrown::HashMap;
use memory::VirtualAddress;
use libc::*;

use crate::unistd::NULL;


lazy_static! {
    /// Stores the layout for each piece of memory allocated by malloc
    static ref MALLOC_LAYOUTS: Mutex<HashMap<VirtualAddress, Layout>> = Mutex::new(HashMap::new());
}

/// Returns a pointer to a portion of heap memory of "size" bytes.
#[no_mangle]
pub unsafe extern "C" fn malloc (size: size_t) -> *mut c_void{
    // alignment requirements of malloc() are not clearly specified.
    const ALIGNMENT: usize = mem::size_of::<usize>();
    let layout =  match Layout::from_size_align(size, ALIGNMENT) {
        Ok(x) => x,
        Err(x) => return NULL,
    };

    let mem = unsafe{ alloc(layout) };

    match VirtualAddress::new(mem as usize) {
        Ok(x) => MALLOC_LAYOUTS.lock().insert(x, layout),
        Err(x) => return NULL,
    };    

    return mem as *mut c_void;
}

/// Deallocates the memory pointed to by "ptr"
#[no_mangle]
pub unsafe extern "C" fn free(ptr: *mut c_void) {
    let layout = match VirtualAddress::new(ptr as usize) {
        Ok(x) =>  MALLOC_LAYOUTS.lock().remove(&x),
        Err(x) => {
            error!("libc::mman::free() could not convert ptr to virtual address: {:?}",x);    
            return;
        }
    };
    match layout {
        Some(x) => dealloc(ptr as *mut u8, x),
        None => {
            error!("libc::mman::free() layout was not found");                
            return;
        }
    }; 
}



