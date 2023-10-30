#![no_std]

extern crate alloc;

use alloc::{borrow::ToOwned, sync::Arc};
use core::mem;

use app_io::{ImmutableRead, ImmutableWrite};
use path::Path;
use rust_ffi::{Error, FatPointer, FfiOption, FfiResult, FfiSlice, FfiSliceMut, FfiStr, FfiString};
use task::{KillReason, TaskRef};

type Result<T> = FfiResult<T, Error>;

// Add the libtheseus:: so mod_mgmt knows which crate to search.

// TODO: Define function types in rust_ffi to guarantee that functions have same
// signature.

fn current_task() -> TaskRef {
    task::get_my_current_task().expect("failed to get current task")
}

#[no_mangle]
#[export_name = "libtheseus::next_u64"]
pub extern "C" fn next_u64() -> u64 {
    random::next_u64()
}

#[no_mangle]
#[export_name = "libtheseus::getcwd"]
pub extern "C" fn getcwd() -> FfiString {
    current_task().get_env().lock().cwd().into()
}

#[no_mangle]
#[export_name = "libtheseus::chdir"]
pub extern "C" fn chdir(path: FfiStr<'_>) -> Result<()> {
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
pub extern "C" fn setenv(key: FfiStr<'_>, value: FfiStr<'_>) -> Result<()> {
    current_task()
        .get_env()
        .lock()
        .set(<&str>::from(key).to_owned(), <&str>::from(value).to_owned());
    Result::Ok(())
}

#[export_name = "libtheseus::unsetenv"]
pub extern "C" fn unsetenv(key: FfiStr<'_>) -> Result<()> {
    current_task().get_env().lock().unset(key.into());
    Result::Ok(())
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

// TODO: Something better than transmutations.
// TODO: Explain why we shouldn't bother using stabby at least for trait objects.

#[export_name = "libtheseus::stdin"]
pub extern "C" fn stdin() -> Result<FatPointer> {
    let ptr: *const dyn ImmutableRead = Arc::into_raw(Result::from(
        app_io::stdin().map_err(|_| Error::BrokenPipe),
    )?);
    Result::Ok(unsafe { mem::transmute(ptr) })
}

#[export_name = "libtheseus::stdout"]
pub extern "C" fn stdout() -> Result<FatPointer> {
    let ptr: *const dyn ImmutableWrite = Arc::into_raw(Result::from(
        app_io::stdout().map_err(|_| Error::BrokenPipe),
    )?);
    Result::Ok(unsafe { mem::transmute(ptr) })
}

#[export_name = "libtheseus::stderr"]
pub extern "C" fn stderr() -> Result<FatPointer> {
    let ptr: *const dyn ImmutableWrite = Arc::into_raw(Result::from(
        app_io::stderr().map_err(|_| Error::BrokenPipe),
    )?);
    Result::Ok(unsafe { mem::transmute(ptr) })
}

#[export_name = "libtheseus::read"]
pub unsafe extern "C" fn read(reader: FatPointer, buf: FfiSliceMut<'_, u8>) -> Result<usize> {
    let ptr: *const dyn ImmutableRead = unsafe { mem::transmute(reader) };
    let r = unsafe { &*ptr };
    Result::from(r.read(buf.into()).map_err(from_core2))
}

#[export_name = "libtheseus::write"]
pub unsafe extern "C" fn write(writer: FatPointer, buf: FfiSlice<'_, u8>) -> Result<usize> {
    let ptr: *const dyn ImmutableWrite = unsafe { mem::transmute(writer) };
    let r = unsafe { &*ptr };
    Result::from(r.write(buf.into()).map_err(from_core2))
}

#[export_name = "libtheseus::flush"]
pub unsafe extern "C" fn flush(_writer: &mut FatPointer) -> Result<()> {
    Result::Ok(())
}

#[export_name = "libtheseus::drop_reader"]
pub unsafe extern "C" fn drop_reader(reader: FatPointer) {
    Arc::<dyn ImmutableRead>::from_raw(unsafe { mem::transmute(reader) });
}

#[export_name = "libtheseus::drop_writer"]
pub unsafe extern "C" fn drop_writer(reader: FatPointer) {
    Arc::<dyn ImmutableWrite>::from_raw(unsafe { mem::transmute(reader) });
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
