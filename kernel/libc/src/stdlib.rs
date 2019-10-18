//! adapted from Redox's stdlib implementation
//! stdlib implementation for Redox, following http://pubs.opengroup.org/onlinepubs/7908799/xsh/stdlib.h.html

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

use crate::{
    types::*,
    errno::*,
    string::*,
    ctype,
    unistd::NULL,
};

pub const RAND_MAX: c_int = 2_147_483_647;

lazy_static! {
    static ref RNG_SAMPLER: Uniform<c_int> = Uniform::new_inclusive(0, RAND_MAX);
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

#[no_mangle]
pub extern "C" fn abs(i: c_int) -> c_int {
    i.abs()
}

macro_rules! dec_num_from_ascii {
    ($s:expr, $t:ty) => {
        unsafe {
            let mut s = $s;
            // Iterate past whitespace
            while ctype::isspace(*s as c_int) != 0 {
                s = s.offset(1);
            }

            // Find out if there is a - sign
            let neg_sign = match *s {
                0x2d => {
                    s = s.offset(1);
                    true
                }
                // '+' increment s and continue parsing
                0x2b => {
                    s = s.offset(1);
                    false
                }
                _ => false,
            };

            let mut n: $t = 0;
            while ctype::isdigit(*s as c_int) != 0 {
                n = 10 * n - (*s as $t - 0x30);
                s = s.offset(1);
            }

            if neg_sign {
                n
            } else {
                -n
            }
        }
    };
}

#[no_mangle]
pub extern "C" fn atoi(s: *const c_char) -> c_int {
    dec_num_from_ascii!(s, c_int)
}

#[no_mangle]
pub extern "C" fn atol(s: *const c_char) -> c_long {
    dec_num_from_ascii!(s, c_long)
}

unsafe extern "C" fn void_cmp(a: *const c_void, b: *const c_void) -> c_int {
    *(a as *const i32) - *(b as *const i32) as c_int
}

