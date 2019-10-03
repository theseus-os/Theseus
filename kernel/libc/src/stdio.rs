//! stdio implementation for Redox, following http://pubs.opengroup.org/onlinepubs/7908799/xsh/stdio.h.html
//! modified Redox implementation

use crate::{c_str::*, types::*, errno::*};
use core::str;

//for know we don't print to a standard error output, we just use the normal log printing
#[no_mangle]
pub unsafe extern "C" fn perror(s: *const c_char) {
    let s_cstr = CStr::from_ptr(s);
    let s_str = str::from_utf8_unchecked(s_cstr.to_bytes());

    if errno >= 0 && errno < STR_ERROR.len() as c_int {
        error!("{}", format_args!("{}: {}\n", s_str, STR_ERROR[errno as usize]));
    } else {
        error!("{}", format_args!("{}: Unknown error {}\n", s_str, errno));
    }
}


// #[no_mangle]
// pub unsafe extern "C" fn sprintf(s: *mut c_char, format: *const c_char, ...) -> c_int {
//     error!("unimplemented");
//     0
// }

// #[no_mangle]
// pub unsafe extern "C" fn fprintf(stream: *mut FILE, format: *const c_char, ...) -> c_int {

// }