use libc::{size_t, c_void};
use errno::*;

use alloc::{
    alloc::{alloc, dealloc, Layout},
    collections::BTreeMap,
};
use spin::Mutex;


/// A map from the set of pointers that have been malloc-ed to the layouts they were malloc-ed with.
static POINTER_LAYOUTS: Mutex<BTreeMap<usize, Layout>> = Mutex::new(BTreeMap::new());


#[no_mangle]
pub unsafe extern "C" fn malloc(size: size_t) -> *mut c_void {
    let layout = match Layout::from_size_align(size, 1) {
        Ok(l)   => l,
        Err(_e) => {
            errno = EINVAL;
            return core::ptr::null_mut();
        }
    };
    let ptr = alloc(layout);
    if ptr.is_null() {
        errno = ENOMEM;
    }
    POINTER_LAYOUTS.lock().insert(ptr as usize, layout);
    ptr as *mut c_void
}


#[no_mangle]
pub unsafe extern "C" fn free(ptr: *mut c_void) {
    if let Some(layout) = POINTER_LAYOUTS.lock().remove(&(ptr as usize)) {
        dealloc(ptr as *mut u8, layout);
    } else {
        error!("free(): failed to free non-malloced pointer {:#X}", ptr as usize);
    }
}


#[no_mangle]
pub unsafe extern "C" fn abort() {
    core::intrinsics::abort();
}
