//! stdio implementation for Redox, following http://pubs.opengroup.org/onlinepubs/7908799/xsh/stdio.h.html
//! modified Redox implementation

use crate::{errno::ERRNO};
use cstr_core::CStr;
use core::str;
use core::sync::atomic::Ordering;
use rlibc::{
    errno::*,
    *
};

//for now we don't print to a standard error output, we just use the normal log printing
#[no_mangle]
pub unsafe extern "C" fn perror(s: *const c_char) {
    let s_cstr = CStr::from_ptr(s);
    let s_str = str::from_utf8_unchecked(s_cstr.to_bytes());
    let errno = ERRNO.load(Ordering::Relaxed);

    if errno >= 0 && errno < STR_ERROR.len() as c_int {
        error!("{}", format_args!("{}: {}\n", s_str, STR_ERROR[errno as usize]));
    } else {
        error!("{}", format_args!("{}: Unknown error {}\n", s_str, errno));
    }
}

