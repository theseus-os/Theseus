//! 

pub mod printf;
use crate::io::{StringWriter, UnsafeStringWriter};

use core::ffi::VaList;
use alloc::vec::Vec;
use libc::{c_char, c_int, size_t};


// #[no_mangle]
// pub unsafe extern "C" fn vfprintf(file: *mut FILE, format: *const c_char, ap: VaList) -> c_int {
//     let mut file = (*file).lock();
//     if let Err(_) = file.try_set_byte_orientation_unlocked() {
//         return -1;
//     }

//     printf::printf(&mut *file, format, ap)
// }

#[no_mangle]
pub unsafe extern "C" fn vprintf(format: *const c_char, ap: VaList) -> c_int {
    let stdout = app_io::stdout().unwrap();
    let mut writer = stdout.lock();
    printf::printf(&mut writer, format, ap)
}

#[no_mangle]
pub unsafe extern "C" fn vasprintf(
    strp: *mut *mut c_char,
    format: *const c_char,
    ap: VaList,
) -> c_int {
    let mut alloc_writer = Vec::new();
    let ret = printf::printf(&mut alloc_writer, format, ap);
    alloc_writer.push(0); // null terminated
    alloc_writer.shrink_to_fit();
    *strp = alloc_writer.leak().as_ptr() as *mut c_char;
    ret
}

#[no_mangle]
pub unsafe extern "C" fn vsnprintf(
    s: *mut c_char,
    n: size_t,
    format: *const c_char,
    ap: VaList,
) -> c_int {
    printf::printf(
        &mut StringWriter(s as *mut u8, n as usize),
        format,
        ap,
    )
}

#[no_mangle]
pub unsafe extern "C" fn vsprintf(s: *mut c_char, format: *const c_char, ap: VaList) -> c_int {
    printf::printf(&mut UnsafeStringWriter(s as *mut u8), format, ap)
}
