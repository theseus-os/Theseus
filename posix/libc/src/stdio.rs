//! stdio implementation for Theseus, following http://pubs.opengroup.org/onlinepubs/7908799/xsh/stdio.h.html

use crate::error::ERRNO;
use cstr_core::{CStr, c_char};
use core::str;
use core::ptr;
use core::sync::atomic::Ordering;
use libc::*;
use fs_node::FileRef;
use alloc::boxed::Box;
use core::ffi::c_void;

//for now we don't print to a standard error output, we just use the normal log printing
// #[no_mangle]
// pub unsafe extern "C" fn perror(s: *const c_char) {
//     let s_cstr = CStr::from_ptr(s);
//     let s_str = str::from_utf8_unchecked(s_cstr.to_bytes());
//     let errno = ERRNO.load(Ordering::Relaxed);

//     if errno >= 0 && errno < STR_ERROR.len() as c_int {
//         error!("{}", format_args!("{}: {}\n", s_str, STR_ERROR[errno as usize]));
//     } else {
//         error!("{}", format_args!("{}: Unknown error {}\n", s_str, errno));
//     }
// }

/// Parse mode flags as a string and output a mode flags integer
unsafe fn parse_mode_flags(mode_str: *const c_char) -> i32 {
    let mut flags = if !strchr(mode_str, b'+' as i32).is_null() {
        O_RDWR
    } else if (*mode_str) == b'r' as i8 {
        O_RDONLY
    } else {
        O_WRONLY
    };
    if !strchr(mode_str, b'x' as i32).is_null() {
        flags |= O_EXCL;
    }
    if !strchr(mode_str, b'e' as i32).is_null() {
        flags |= O_CLOEXEC;
    }
    if (*mode_str) != b'r' as i8 {
        flags |= O_CREAT;
    }
    if (*mode_str) == b'w' as i8 {
        flags |= O_TRUNC;
    } else if (*mode_str) == b'a' as i8 {
        flags |= O_APPEND;
    }

    flags
}

pub struct FILE {
    fd: c_int,
}

/// Opens a stream to the file at path `filename` with the given `mode`.
/// Returns a struct containing the stream information. This struct can
/// be used with other methods to manipulate the opened file.
///
/// Internally uses [`open`](../fs/fn.open.html), and hence depends on its
/// implementation for POSIX compliance.
#[no_mangle]
pub unsafe extern "C" fn fopen(filename: *const c_char, mode: *const c_char) -> *mut FILE {
    const CREAT_MODE: i32 = 0o666;

    let initial_mode = *mode;
    if initial_mode != b'r' as i8 && initial_mode != b'w' as i8 && initial_mode != b'a' as i8 {
        error!("libc::stdio::fopen(): Invalid mode");
        ERRNO.store(EINVAL, Ordering::Relaxed);
        return ptr::null_mut();
    }

    let flags = parse_mode_flags(mode);
    let new_mode = if flags & O_CREAT == O_CREAT {
        CREAT_MODE
    } else {
        0
    };

    let fd = ::fs::open(CStr::from_ptr(filename), flags, new_mode);

    if fd < 0 {
        error!("libc::stdio::fopen(): Unable to retrieve descriptor for file");
        ERRNO.store(EINVAL, Ordering::Relaxed);
        return ptr::null_mut();
    }

    Box::into_raw(Box::new(FILE {
        fd,
    }))
}

/// Closes the `stream` and destroys the `FILE` reference. If this is the
/// last stream referring to a file, the file is deleted. The `stream`
/// pointer is rendered invalid after this method is called.
///
/// Internally uses [`close`](../fs/fn.close.html), and hence depends on its
/// implementation for POSIX compliance.
#[no_mangle]
pub unsafe extern "C" fn fclose(stream: *mut FILE) -> c_int {
    let ret = ::fs::close((*stream).fd);

    if ret != 0 {
        error!("libc::stdio::fclose(): Unable to close file");
        ERRNO.store(EINVAL, Ordering::Relaxed);
        return ret;
    }

    // Destroy stream
    Box::from_raw(stream);

    0
}

/// Reads `nitems` items each `size` bytes long from `stream` into `ptr`. Returns
/// the number of items read.
///
/// Internally uses [`read`](../fs/fn.read.html), and hence depends on its
/// implementation for POSIX compliance.
#[no_mangle]
pub unsafe extern "C" fn fread(ptr: *mut c_void, size: size_t, nitems: size_t,
                               stream: *mut FILE) -> size_t {
    let count = size * nitems;

    if count == 0 {
        return 0;
    }

    let ret = ::fs::read((*stream).fd, ptr, count);

    if ret < 0 {
        error!("libc::stdio::fread(): Error in reading file");
        ERRNO.store(EINVAL, Ordering::Relaxed);
        return 0;
    }

    (ret as size_t / size)
}

/// Writes `nitems` items each `size` bytes long from `ptr` to `stream`. Returns
/// the number of items written.
///
/// Internally uses [`write`](../fs/fn.write.html), and hence depends on its
/// implementation for POSIX compliance.
#[no_mangle]
pub unsafe extern "C" fn fwrite(ptr: *const c_void, size: size_t, nitems: size_t,
                                stream: *mut FILE) -> size_t {
    let count = size * nitems;

    if count == 0 {
        return 0;
    }

    let ret = ::fs::write((*stream).fd, ptr, count);

    if ret < 0 {
        error!("libc::stdio::fwrite(): Error in writing to file");
        ERRNO.store(EINVAL, Ordering::Relaxed);
        return 0;
    }

    (ret as size_t / size)
}
