use core::ptr;
use alloc::vec::Vec;
use libc::{c_char};

pub static mut argv: *mut *mut c_char = ptr::null_mut();
pub static mut inner_argv: Vec<*mut c_char> = Vec::new();

/// Externally-accessible from C
#[no_mangle]
pub static mut environ: *mut *mut c_char = ptr::null_mut();
pub static mut inner_environ: Vec<*mut c_char> = Vec::new();



#[no_mangle]
pub unsafe extern "C" fn __program_invocation_name() -> *mut *mut c_char {
    &mut inner_argv[0]
}
