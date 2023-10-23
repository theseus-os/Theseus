#![no_std]
#![feature(vec_into_raw_parts)]

extern crate alloc;

use alloc::string::String;
use core::{marker::PhantomData, slice, str};

use path::Path;
use task::get_my_current_task;

// Add the libtheseus:: so mod_mgmt knows which crate to search.

#[repr(C)]
pub struct FfiString {
    buf: *mut u8,
    length: usize,
    capacity: usize,
}

impl From<String> for FfiString {
    fn from(value: String) -> Self {
        let (buf, length, capacity) = value.into_raw_parts();
        Self {
            buf,
            length,
            capacity,
        }
    }
}

impl From<FfiString> for String {
    fn from(
        FfiString {
            buf,
            length,
            capacity,
        }: FfiString,
    ) -> Self {
        unsafe { Self::from_raw_parts(buf, length, capacity) }
    }
}

#[repr(C)]
pub struct FfiStr<'a> {
    buf: *const u8,
    length: usize,
    _phantom_data: PhantomData<&'a ()>,
}

impl<'a> From<&'a str> for FfiStr<'a> {
    fn from(value: &'a str) -> Self {
        let buf = value.as_bytes();
        Self {
            buf: buf.as_ptr(),
            length: buf.len(),
            _phantom_data: PhantomData,
        }
    }
}

impl<'a> From<FfiStr<'a>> for &'a str {
    fn from(FfiStr { buf, length, .. }: FfiStr<'a>) -> Self {
        let bytes = unsafe { slice::from_raw_parts(buf, length) };
        unsafe { str::from_utf8_unchecked(bytes) }
    }
}

#[no_mangle]
#[export_name = "libtheseus::next_u64"]
pub extern "C" fn next_u64() -> u64 {
    random::next_u64()
}

#[no_mangle]
#[export_name = "libtheseus::getcwd"]
pub extern "C" fn getcwd() -> FfiString {
    get_my_current_task().unwrap().get_env().lock().cwd().into()
}

#[no_mangle]
#[export_name = "libtheseus::chdir"]
pub extern "C" fn chdir(path: FfiStr<'_>) {
    get_my_current_task()
        .unwrap()
        .get_env()
        .lock()
        .chdir(Path::new(path.into()));
}
