#![no_std]
#![feature(extern_types)]

#[cfg(not(feature = "rustc-dep-of-std"))]
extern crate alloc;

use alloc::string::String;

pub use rust_ffi::Error;
use rust_ffi::{FatPointer, FfiString};

type Result<T> = core::result::Result<T, Error>;

mod c {
    use rust_ffi::{
        Error, FatPointer, FfiOption, FfiResult, FfiSlice, FfiSliceMut, FfiStr, FfiString,
    };

    #[link(name = "libtheseus")]
    extern "C" {
        #[link_name = "libtheseus::next_u64"]
        pub(crate) fn next_u64() -> u64;

        #[link_name = "libtheseus::getcwd"]
        pub(crate) fn getcwd() -> FfiString;

        #[link_name = "libtheseus::chdir"]
        pub(crate) fn chdir(path: FfiStr<'_>);

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
        pub(crate) fn stdin() -> FatPointer;

        #[link_name = "libtheseus::stdout"]
        pub(crate) fn stdout() -> FatPointer;

        #[link_name = "libtheseus::stderr"]
        pub(crate) fn stderr() -> FatPointer;

        // TODO: Mutable reference?

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
}

pub fn next_u64() -> u64 {
    unsafe { c::next_u64() }
}

pub fn getcwd() -> String {
    unsafe { c::getcwd() }.into()
}

pub fn chdir(path: &str) {
    let path = path.into();
    unsafe { c::chdir(path) };
}

pub fn getenv(key: &str) -> Option<String> {
    let key = key.into();
    Into::<Option<FfiString>>::into(unsafe { c::getenv(key) }).map(|s| s.into())
}

pub fn setenv(key: &str, value: &str) -> Result<()> {
    let key = key.into();
    let value = value.into();
    unsafe { c::setenv(key, value) }.into()
}

pub fn unsetenv(key: &str) -> Result<()> {
    let key = key.into();
    unsafe { c::unsetenv(key) }.into()
}

pub fn exit(code: i32) -> ! {
    unsafe { c::exit(code) };
}

pub fn getpid() -> u32 {
    unsafe { c::getpid() }
}

pub unsafe fn register_dtor(t: *mut u8, dtor: unsafe extern "C" fn(*mut u8)) {
    c::register_dtor(t, dtor)
}

// TODO: impl Send + Sync for Reader and Writer?

pub struct Reader {
    inner: FatPointer,
}

impl Drop for Reader {
    fn drop(&mut self) {
        unsafe { c::drop_reader(self.inner.clone()) };
    }
}

pub struct Writer {
    inner: FatPointer,
}

impl Drop for Writer {
    fn drop(&mut self) {
        unsafe { c::drop_writer(self.inner.clone()) };
    }
}

pub fn stdin() -> Reader {
    Reader {
        inner: unsafe { c::stdin() },
    }
}

pub fn stdout() -> Writer {
    Writer {
        inner: unsafe { c::stdout() },
    }
}

pub fn stderr() -> Writer {
    Writer {
        inner: unsafe { c::stderr() },
    }
}

// TODO: Mutable reference?

pub fn read(reader: &mut Reader, buf: &mut [u8]) -> Result<usize> {
    unsafe { c::read(reader.inner.clone(), buf.into()) }.into()
}

pub fn write(writer: &mut Writer, buf: &[u8]) -> Result<usize> {
    unsafe { c::write(writer.inner.clone(), buf.into()) }.into()
}

pub fn flush(writer: &mut Writer) -> Result<()> {
    unsafe { c::flush(writer.inner.clone()) }.into()
}
