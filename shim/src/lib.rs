//! Provides Theseus OS functionality without a direct dependency on Theseus
//! kernel crates.
//!
//! It does so by declaring functions that are implemented by the `libtheseus`
//! kernel crate. These functions are referenced as relocations in the dependent
//! (i.e. `std`) object file that `mod_mgmt` then fills in at runtime.

#![no_std]
#![feature(extern_types)]

#[cfg(not(feature = "rustc-dep-of-std"))]
extern crate alloc;

use alloc::string::String;

pub use theseus_ffi::Error;
use theseus_ffi::{FatPointer, FfiString};

type Result<T> = core::result::Result<T, Error>;

mod c {
    use theseus_ffi::{
        Error, FatPointer, FfiOption, FfiResult, FfiSlice, FfiSliceMut, FfiStr, FfiString,
    };

    #[link(name = "libtheseus")]
    extern "C" {
        #[link_name = "libtheseus::next_u64"]
        pub(crate) fn next_u64() -> u64;

        #[link_name = "libtheseus::getcwd"]
        pub(crate) fn getcwd() -> FfiString;

        #[link_name = "libtheseus::chdir"]
        pub(crate) fn chdir(path: FfiStr<'_>) -> FfiResult<(), Error>;

        #[link_name = "libtheseus::getenv"]
        pub(crate) fn getenv(key: FfiStr<'_>) -> FfiOption<FfiString>;

        #[link_name = "libtheseus::setenv"]
        pub(crate) fn setenv(key: FfiStr<'_>, value: FfiStr<'_>) -> FfiResult<(), Error>;

        #[link_name = "libtheseus::unsetenv"]
        pub(crate) fn unsetenv(key: FfiStr<'_>) -> FfiResult<(), Error>;

        #[link_name = "libtheseus::exit"]
        pub(crate) fn exit(code: i32) -> !;

        #[link_name = "libtheseus::getpid"]
        pub(crate) fn getpid() -> u32;

        #[link_name = "libtheseus::register_dtor"]
        pub(crate) fn register_dtor(t: *mut u8, dtor: unsafe extern "C" fn(*mut u8));

        #[link_name = "libtheseus::stdin"]
        pub(crate) fn stdin() -> FfiResult<FatPointer, Error>;

        #[link_name = "libtheseus::stdout"]
        pub(crate) fn stdout() -> FfiResult<FatPointer, Error>;

        #[link_name = "libtheseus::stderr"]
        pub(crate) fn stderr() -> FfiResult<FatPointer, Error>;

        #[link_name = "libtheseus::read"]
        pub(crate) fn read(reader: FatPointer, buf: FfiSliceMut<'_, u8>)
            -> FfiResult<usize, Error>;

        #[link_name = "libtheseus::write"]
        pub(crate) fn write(writer: FatPointer, buf: FfiSlice<'_, u8>) -> FfiResult<usize, Error>;

        #[link_name = "libtheseus::flush"]
        pub(crate) fn flush(writer: FatPointer) -> FfiResult<(), Error>;

        #[link_name = "libtheseus::drop_reader"]
        pub(crate) fn drop_reader(reader: FatPointer);

        #[link_name = "libtheseus::drop_writer"]
        pub(crate) fn drop_writer(writer: FatPointer);
    }

    const _: theseus_ffi::next_u64 = next_u64;
    const _: theseus_ffi::getcwd = getcwd;
    const _: theseus_ffi::chdir = chdir;
    const _: theseus_ffi::getenv = getenv;
    const _: theseus_ffi::setenv = setenv;
    const _: theseus_ffi::unsetenv = unsetenv;
    const _: theseus_ffi::exit = exit;
    const _: theseus_ffi::getpid = getpid;
    const _: theseus_ffi::register_dtor = register_dtor;
    const _: theseus_ffi::stdin = stdin;
    const _: theseus_ffi::stdout = stdout;
    const _: theseus_ffi::stderr = stderr;
    const _: theseus_ffi::read = read;
    const _: theseus_ffi::write = write;
    const _: theseus_ffi::flush = flush;
    const _: theseus_ffi::drop_reader = drop_reader;
    const _: theseus_ffi::drop_writer = drop_writer;
}

#[inline]
pub fn next_u64() -> u64 {
    unsafe { c::next_u64() }
}

#[inline]
pub fn getcwd() -> String {
    unsafe { c::getcwd() }.into()
}

#[inline]
pub fn chdir(path: &str) -> Result<()> {
    let path = path.into();
    unsafe { c::chdir(path) }.into()
}

#[inline]
pub fn getenv(key: &str) -> Option<String> {
    let key = key.into();
    Into::<Option<FfiString>>::into(unsafe { c::getenv(key) }).map(|s| s.into())
}

#[inline]
pub fn setenv(key: &str, value: &str) -> Result<()> {
    let key = key.into();
    let value = value.into();
    unsafe { c::setenv(key, value) }.into()
}

#[inline]
pub fn unsetenv(key: &str) -> Result<()> {
    let key = key.into();
    unsafe { c::unsetenv(key) }.into()
}

#[inline]
pub fn exit(code: i32) -> ! {
    unsafe { c::exit(code) }
}

#[inline]
pub fn getpid() -> u32 {
    unsafe { c::getpid() }
}

#[inline]
pub unsafe fn register_dtor(t: *mut u8, dtor: unsafe extern "C" fn(*mut u8)) {
    c::register_dtor(t, dtor)
}

pub struct Reader {
    inner: FatPointer,
}

impl Drop for Reader {
    #[inline]
    fn drop(&mut self) {
        unsafe { c::drop_reader(self.inner.clone()) }
    }
}

pub struct Writer {
    inner: FatPointer,
}

impl Drop for Writer {
    #[inline]
    fn drop(&mut self) {
        unsafe { c::drop_writer(self.inner.clone()) }
    }
}

#[inline]
pub fn stdin() -> Result<Reader> {
    Ok(Reader {
        inner: Result::from(unsafe { c::stdin() })?,
    })
}

#[inline]
pub fn stdout() -> Result<Writer> {
    Ok(Writer {
        inner: Result::from(unsafe { c::stdout() })?,
    })
}

#[inline]
pub fn stderr() -> Result<Writer> {
    Ok(Writer {
        inner: Result::from(unsafe { c::stderr() })?,
    })
}

// NOTE: The references below don't need to be mutable.

#[inline]
pub fn read(reader: &mut Reader, buf: &mut [u8]) -> Result<usize> {
    unsafe { c::read(reader.inner.clone(), buf.into()) }.into()
}

#[inline]
pub fn write(writer: &mut Writer, buf: &[u8]) -> Result<usize> {
    unsafe { c::write(writer.inner.clone(), buf.into()) }.into()
}

#[inline]
pub fn flush(writer: &mut Writer) -> Result<()> {
    unsafe { c::flush(writer.inner.clone()) }.into()
}
