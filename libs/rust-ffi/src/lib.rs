#![no_std]
#![feature(vec_into_raw_parts, try_trait_v2, never_type, exhaustive_patterns)]
#![allow(non_camel_case_types)]

#[cfg(not(feature = "rustc-dep-of-std"))]
extern crate alloc;

use alloc::string::String;
use core::{
    convert::Infallible,
    marker::PhantomData,
    ops::{ControlFlow, FromResidual, Try},
    str,
};

pub type next_u64 = unsafe extern "C" fn() -> u64;
pub type getcwd = unsafe extern "C" fn() -> FfiString;
pub type chdir = unsafe extern "C" fn(path: FfiStr<'_>) -> FfiResult<(), Error> ;
pub type getenv = unsafe extern "C" fn(key: FfiStr<'_>) -> FfiOption<FfiString>;
pub type setenv = unsafe extern "C" fn(key: FfiStr<'_>, value: FfiStr<'_>) -> FfiResult<(), Error>;
pub type unsetenv = unsafe extern "C" fn(key: FfiStr<'_>) -> FfiResult<(), Error>;
pub type exit = unsafe extern "C" fn(code: i32) -> !;
pub type getpid = unsafe extern "C" fn() -> u32;
pub type register_dtor = unsafe extern "C" fn(t: *mut u8, dtor: unsafe extern "C" fn(*mut u8));
pub type stdin = unsafe extern "C" fn() -> FfiResult<FatPointer, Error>;
pub type stdout = unsafe extern "C" fn() -> FfiResult<FatPointer, Error>;
pub type stderr = unsafe extern "C" fn() -> FfiResult<FatPointer, Error>;
pub type read = unsafe extern "C" fn(reader: FatPointer, buf: FfiSliceMut<'_, u8>) -> FfiResult<usize, Error>;
pub type write = unsafe extern "C" fn(writer: FatPointer, buf: FfiSlice<'_, u8>) -> FfiResult<usize, Error>;
pub type flush = unsafe extern "C" fn(writer: FatPointer) -> FfiResult<(), Error>;
pub type drop_reader = unsafe extern "C" fn(reader: FatPointer);
pub type drop_writer = unsafe extern "C" fn(writer: FatPointer);

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
    inner: FfiSlice<'a, u8>,
}

impl<'a> From<&'a str> for FfiStr<'a> {
    fn from(value: &'a str) -> Self {
        Self {
            inner: value.as_bytes().into(),
        }
    }
}

impl<'a> From<FfiStr<'a>> for &'a str {
    fn from(FfiStr { inner }: FfiStr<'a>) -> Self {
        unsafe { str::from_utf8_unchecked(inner.into()) }
    }
}

#[repr(C)]
pub struct FfiStrMut<'a> {
    inner: FfiSliceMut<'a, u8>,
}

impl<'a> From<&'a mut str> for FfiStrMut<'a> {
    fn from(value: &'a mut str) -> Self {
        Self {
            inner: unsafe { value.as_bytes_mut() }.into(),
        }
    }
}

impl<'a> From<FfiStrMut<'a>> for &'a mut str {
    fn from(FfiStrMut { inner }: FfiStrMut<'a>) -> Self {
        unsafe { str::from_utf8_unchecked_mut(inner.into()) }
    }
}

#[repr(C)]
pub struct FfiSlice<'a, T> {
    buf: *const T,
    length: usize,
    _phantom_data: PhantomData<&'a T>,
}

impl<'a, T> From<&'a [T]> for FfiSlice<'a, T> {
    fn from(value: &'a [T]) -> Self {
        Self {
            buf: value.as_ptr(),
            length: value.len(),
            _phantom_data: PhantomData,
        }
    }
}

impl<'a, T> From<FfiSlice<'a, T>> for &'a [T] {
    fn from(value: FfiSlice<'a, T>) -> Self {
        unsafe { core::slice::from_raw_parts(value.buf, value.length) }
    }
}

#[repr(C)]
pub struct FfiSliceMut<'a, T> {
    buf: *mut T,
    length: usize,
    _phantom_data: PhantomData<&'a T>,
}

impl<'a, T> From<&'a mut [T]> for FfiSliceMut<'a, T> {
    fn from(value: &'a mut [T]) -> Self {
        Self {
            buf: value.as_mut_ptr(),
            length: value.len(),
            _phantom_data: PhantomData,
        }
    }
}

impl<'a, T> From<FfiSliceMut<'a, T>> for &'a mut [T] {
    fn from(value: FfiSliceMut<'a, T>) -> Self {
        unsafe { core::slice::from_raw_parts_mut(value.buf, value.length) }
    }
}

#[repr(C)]
pub enum FfiResult<T, E> {
    Ok(T),
    Err(E),
}

impl<T, E> From<FfiResult<T, E>> for Result<T, E> {
    fn from(value: FfiResult<T, E>) -> Self {
        match value {
            FfiResult::Ok(t) => Ok(t),
            FfiResult::Err(e) => Err(e),
        }
    }
}

impl<T, E> From<Result<T, E>> for FfiResult<T, E> {
    fn from(value: Result<T, E>) -> Self {
        match value {
            Ok(t) => FfiResult::Ok(t),
            Err(e) => FfiResult::Err(e),
        }
    }
}

impl<T, E> Try for FfiResult<T, E> {
    type Output = T;

    type Residual = FfiResult<Infallible, E>;

    fn from_output(output: Self::Output) -> Self {
        Self::Ok(output)
    }

    fn branch(self) -> ControlFlow<Self::Residual, Self::Output> {
        match self {
            Self::Ok(v) => ControlFlow::Continue(v),
            Self::Err(e) => ControlFlow::Break(FfiResult::Err(e)),
        }
    }
}

impl<T, E> FromResidual for FfiResult<T, E> {
    fn from_residual(residual: <Self as core::ops::Try>::Residual) -> Self {
        match residual {
            FfiResult::Err(e) => Self::Err(e),
        }
    }
}

#[repr(C)]
pub enum FfiOption<T> {
    Some(T),
    None,
}

impl<T> From<FfiOption<T>> for Option<T> {
    fn from(value: FfiOption<T>) -> Self {
        match value {
            FfiOption::Some(t) => Some(t),
            FfiOption::None => None,
        }
    }
}

impl<T> From<Option<T>> for FfiOption<T> {
    fn from(value: Option<T>) -> Self {
        match value {
            Some(t) => FfiOption::Some(t),
            None => FfiOption::None,
        }
    }
}

impl<T> Try for FfiOption<T> {
    type Output = T;

    type Residual = FfiOption<Infallible>;

    fn from_output(output: Self::Output) -> Self {
        Self::Some(output)
    }

    fn branch(self) -> ControlFlow<Self::Residual, Self::Output> {
        match self {
            Self::Some(v) => ControlFlow::Continue(v),
            Self::None => ControlFlow::Break(FfiOption::None),
        }
    }
}

impl<T> FromResidual for FfiOption<T> {
    fn from_residual(residual: <Self as core::ops::Try>::Residual) -> Self {
        match residual {
            FfiOption::None => Self::None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub enum Error {
    NotFound,
    PermissionDenied,
    ConnectionRefused,
    ConnectionReset,
    HostUnreachable,
    NetworkUnreachable,
    ConnectionAborted,
    NotConnected,
    AddrInUse,
    AddrNotAvailable,
    NetworkDown,
    BrokenPipe,
    AlreadyExists,
    WouldBlock,
    NotADirectory,
    IsADirectory,
    DirectoryNotEmpty,
    ReadOnlyFilesystem,
    FilesystemLoop,
    StaleNetworkFileHandle,
    InvalidInput,
    InvalidData,
    TimedOut,
    WriteZero,
    StorageFull,
    NotSeekable,
    FilesystemQuotaExceeded,
    FileTooLarge,
    ResourceBusy,
    ExecutableFileBusy,
    Deadlock,
    CrossesDevices,
    TooManyLinks,
    InvalidFilename,
    ArgumentListTooLong,
    Interrupted,
    Unsupported,
    UnexpectedEof,
    OutOfMemory,
    Other,
}

#[derive(Debug, Clone)]
#[repr(C)]
pub struct FatPointer {
    _a: *mut (),
    _b: *mut (),
}

const _FAT_POINTER_SIZE: () =
    assert!(core::mem::size_of::<FatPointer>() == core::mem::size_of::<*const dyn core::fmt::Debug>());
