use cstr_core::c_char;
use core::ptr;
use libc::c_int;

/// Returns a pointer to the first occurrence of `c` in the C string `s`. If
/// `c` is not found in `s`, a null pointer is returned.
//
// The terminating null-character is considered part of the C string. Therefore,
// it can also be located in order to retrieve a pointer to the end of a string.
#[no_mangle]
pub unsafe extern "C" fn strchr(mut s: *const c_char, c: c_int) -> *mut c_char {
    let c = c as c_char;
    while *s != 0 {
        if *s == c {
            return s as *mut c_char;
        }
        s = s.offset(1);
    }
    ptr::null_mut()
}