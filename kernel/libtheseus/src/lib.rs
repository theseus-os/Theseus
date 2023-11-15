//! This crate implements the functions defined in `shim`.
//!
//! See the `shim` crate for more details.

#![no_std]
#![allow(clippy::missing_safety_doc)]

extern crate alloc;

use alloc::{borrow::ToOwned, sync::Arc};
use core::mem;

use app_io::{ImmutableRead, ImmutableWrite};
use path::Path;
use task::{KillReason, TaskRef};
use theseus_ffi::{
    Error, FatPointer, FfiOption, FfiResult, FfiSlice, FfiSliceMut, FfiStr, FfiString,
};

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

fn current_task() -> TaskRef {
    task::get_my_current_task().expect("failed to get current task")
}

// Add the libtheseus:: prefix so mod_mgmt knows which crate to search.

#[export_name = "libtheseus::next_u64"]
pub extern "C" fn next_u64() -> u64 {
    random::next_u64()
}

#[export_name = "libtheseus::getcwd"]
pub extern "C" fn getcwd() -> FfiString {
    current_task().get_env().lock().cwd().into()
}

#[export_name = "libtheseus::chdir"]
pub extern "C" fn chdir(path: FfiStr<'_>) -> FfiResult<(), Error> {
    current_task()
        .get_env()
        .lock()
        .chdir(Path::new(path.into()))
        .map_err(|e| match e {
            environment::Error::NotADirectory => Error::NotADirectory,
            environment::Error::NotFound => Error::NotFound,
        })
        .into()
}

#[export_name = "libtheseus::getenv"]
pub extern "C" fn getenv(key: FfiStr<'_>) -> FfiOption<FfiString> {
    current_task()
        .get_env()
        .lock()
        .get(key.into())
        .map(|s| s.to_owned().into())
        .into()
}

#[export_name = "libtheseus::setenv"]
pub extern "C" fn setenv(key: FfiStr<'_>, value: FfiStr<'_>) -> FfiResult<(), Error> {
    current_task()
        .get_env()
        .lock()
        .set(<&str>::from(key).to_owned(), <&str>::from(value).to_owned());
    FfiResult::Ok(())
}

#[export_name = "libtheseus::unsetenv"]
pub extern "C" fn unsetenv(key: FfiStr<'_>) -> FfiResult<(), Error> {
    current_task().get_env().lock().unset(key.into());
    FfiResult::Ok(())
}

#[export_name = "libtheseus::exit"]
pub extern "C" fn exit(_code: i32) -> ! {
    // TODO: Supply correct reason.
    current_task()
        .kill(KillReason::Requested)
        .expect("couldn't mark task as exited");
    task::schedule();
    panic!("task scheduled after exiting");
}

#[export_name = "libtheseus::getpid"]
pub extern "C" fn getpid() -> u32 {
    task::get_my_current_task_id()
        .try_into()
        .expect("current task id too large")
}

#[export_name = "libtheseus::register_dtor"]
pub unsafe extern "C" fn register_dtor(t: *mut u8, dtor: unsafe extern "C" fn(*mut u8)) {
    unsafe { thread_local_macro::register_dtor(t, dtor) }
}

// TODO: Something better than transmutations?

// One might naively assume that we are better off using `stabby` trait objects,
// but that means throughtout Theseus we would have to use
// stabby::alloc::sync::Arc and vtable!(Trait), rather than alloc::sync::Arc and
// dyn Trait respectively. The current solution isn't great, but it's only funky
// at the boundary to `shim` and doesn't impact the rest of Theseus.

#[export_name = "libtheseus::stdin"]
pub extern "C" fn stdin() -> FfiResult<FatPointer, Error> {
    let ptr: *const dyn ImmutableRead = Arc::into_raw(FfiResult::from(
        app_io::stdin().map_err(|_| Error::BrokenPipe),
    )?);
    FfiResult::Ok(unsafe { mem::transmute(ptr) })
}

#[export_name = "libtheseus::stdout"]
pub extern "C" fn stdout() -> FfiResult<FatPointer, Error> {
    let ptr: *const dyn ImmutableWrite = Arc::into_raw(FfiResult::from(
        app_io::stdout().map_err(|_| Error::BrokenPipe),
    )?);
    FfiResult::Ok(unsafe { mem::transmute(ptr) })
}

#[export_name = "libtheseus::stderr"]
pub extern "C" fn stderr() -> FfiResult<FatPointer, Error> {
    let ptr: *const dyn ImmutableWrite = Arc::into_raw(FfiResult::from(
        app_io::stderr().map_err(|_| Error::BrokenPipe),
    )?);
    FfiResult::Ok(unsafe { mem::transmute(ptr) })
}

#[export_name = "libtheseus::read"]
pub unsafe extern "C" fn read(
    reader: FatPointer,
    buf: FfiSliceMut<'_, u8>,
) -> FfiResult<usize, Error> {
    let ptr: *const dyn ImmutableRead = unsafe { mem::transmute(reader) };
    let r = unsafe { &*ptr };
    FfiResult::from(r.read(buf.into()).map_err(from_core2))
}

#[export_name = "libtheseus::write"]
pub unsafe extern "C" fn write(
    writer: FatPointer,
    buf: FfiSlice<'_, u8>,
) -> FfiResult<usize, Error> {
    let ptr: *const dyn ImmutableWrite = unsafe { mem::transmute(writer) };
    let r = unsafe { &*ptr };
    FfiResult::from(r.write(buf.into()).map_err(from_core2))
}

#[export_name = "libtheseus::flush"]
pub unsafe extern "C" fn flush(_writer: FatPointer) -> FfiResult<(), Error> {
    FfiResult::Ok(())
}

#[export_name = "libtheseus::drop_reader"]
pub unsafe extern "C" fn drop_reader(reader: FatPointer) {
    Arc::<dyn ImmutableRead>::from_raw(unsafe { mem::transmute(reader) });
}

#[export_name = "libtheseus::drop_writer"]
pub unsafe extern "C" fn drop_writer(writer: FatPointer) {
    Arc::<dyn ImmutableWrite>::from_raw(unsafe { mem::transmute(writer) });
}

fn from_core2(e: core2::io::Error) -> Error {
    match e.kind() {
        core2::io::ErrorKind::NotFound => Error::NotFound,
        core2::io::ErrorKind::PermissionDenied => Error::PermissionDenied,
        core2::io::ErrorKind::ConnectionRefused => Error::ConnectionRefused,
        core2::io::ErrorKind::ConnectionReset => Error::ConnectionReset,
        core2::io::ErrorKind::ConnectionAborted => Error::ConnectionAborted,
        core2::io::ErrorKind::NotConnected => Error::NotConnected,
        core2::io::ErrorKind::AddrInUse => Error::AddrInUse,
        core2::io::ErrorKind::AddrNotAvailable => Error::AddrNotAvailable,
        core2::io::ErrorKind::BrokenPipe => Error::BrokenPipe,
        core2::io::ErrorKind::AlreadyExists => Error::AlreadyExists,
        core2::io::ErrorKind::WouldBlock => Error::WouldBlock,
        core2::io::ErrorKind::InvalidInput => Error::InvalidInput,
        core2::io::ErrorKind::InvalidData => Error::InvalidData,
        core2::io::ErrorKind::TimedOut => Error::TimedOut,
        core2::io::ErrorKind::WriteZero => Error::WriteZero,
        core2::io::ErrorKind::Interrupted => Error::Interrupted,
        core2::io::ErrorKind::UnexpectedEof => Error::UnexpectedEof,
        core2::io::ErrorKind::Other => Error::Other,
        _ => Error::Other,
    }
}
