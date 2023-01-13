//! WASI system call, signature, and permission definitions as well as mappings.
//!
//! This module contains the following:
//! * Macros for easily defining wasmi function signatures.
//! * SystemCall enum type consisting of supported system calls.
//! * Mapping from system call string to SystemCall type.
//! * Mapping between system call number and SystemCall type.
//! * Mapping from SystemCall type to wasmi signature.
//! * Definitions of WASI rights for full file and directory permissions.
//!
//! Signature macro from tomaka/redshirt:
//! <https://github.com/tomaka/redshirt/blob/4df506f68821353a7fd67bb94c4223df6b683e1b/kernel/core/src/primitives.rs>
//!

use alloc::vec::Vec;
use core::convert::TryFrom;
use core::str::FromStr;
use wasmi::{Signature, ValueType};

/// Generates wasmi function signature.
///
/// # Arguments
/// * `params`: function signature argument types.
/// * `ret_ty`: function signature return type.
///
/// # Return
/// Returns requested wasmi signature.
pub fn get_signature(
    params: impl Iterator<Item = ValueType>,
    ret_ty: impl Into<Option<ValueType>>,
) -> Signature {
    wasmi::Signature::new(
        params.map(wasmi::ValueType::from).collect::<Vec<_>>(),
        ret_ty.into().map(wasmi::ValueType::from),
    )
}

/// Macro to efficiently generate wasmi function signature.
///
/// Usage examples:
///     sig!((I32))
///     sig!((I32, I32)->I32)
///
#[macro_export]
macro_rules! sig {
    (($($p:ident),*)) => {{
        let params = core::iter::empty();
        $(let params = params.chain(core::iter::once(ValueType::$p));)*
        $crate::wasi_definitions::get_signature(params, None)
    }};
    (($($p:ident),*) -> $ret:ident) => {{
        let params = core::iter::empty();
        $(let params = params.chain(core::iter::once(ValueType::$p));)*
        $crate::wasi_definitions::get_signature(params, Some($crate::ValueType::$ret))
    }};
}

/// WASI system calls that are currently supported.
#[derive(Copy, Clone, Debug)]
pub enum SystemCall {
    ProcExit,
    FdClose,
    FdWrite,
    FdSeek,
    FdRead,
    FdFdstatGet,
    EnvironSizesGet,
    EnvironGet,
    FdPrestatGet,
    FdPrestatDirName,
    PathOpen,
    FdFdstatSetFlags,
    ArgsSizesGet,
    ArgsGet,
    ClockTimeGet,
}

impl FromStr for SystemCall {
    type Err = &'static str;

    /// Get SystemCall type from imported function name string.
    ///
    /// # Arguments:
    /// * `fn_name`: system call string representation.
    ///
    /// # Return
    /// Returns SystemCall enum corresponding to given system call string.
    fn from_str(fn_name: &str) -> Result<Self, Self::Err> {
        match fn_name {
            "proc_exit" => Ok(SystemCall::ProcExit),
            "fd_close" => Ok(SystemCall::FdClose),
            "fd_write" => Ok(SystemCall::FdWrite),
            "fd_seek" => Ok(SystemCall::FdSeek),
            "fd_read" => Ok(SystemCall::FdRead),
            "fd_fdstat_get" => Ok(SystemCall::FdFdstatGet),
            "environ_sizes_get" => Ok(SystemCall::EnvironSizesGet),
            "environ_get" => Ok(SystemCall::EnvironGet),
            "fd_prestat_get" => Ok(SystemCall::FdPrestatGet),
            "fd_prestat_dir_name" => Ok(SystemCall::FdPrestatDirName),
            "path_open" => Ok(SystemCall::PathOpen),
            "fd_fdstat_set_flags" => Ok(SystemCall::FdFdstatSetFlags),
            "args_sizes_get" => Ok(SystemCall::ArgsSizesGet),
            "args_get" => Ok(SystemCall::ArgsGet),
            "clock_time_get" => Ok(SystemCall::ClockTimeGet),
            _ => Err("Unknown WASI system call."),
        }
    }
}

impl TryFrom<usize> for SystemCall {
    type Error = &'static str;

    /// Get SystemCall type from system call number.
    ///
    /// # Arguments:
    /// * `syscall_index`: system call number.
    ///
    /// # Return
    /// Returns SystemCall enum corresponding to given system call number.
    fn try_from(syscall_index: usize) -> Result<Self, Self::Error> {
        match syscall_index {
            0 => Ok(SystemCall::ProcExit),
            1 => Ok(SystemCall::FdClose),
            2 => Ok(SystemCall::FdWrite),
            3 => Ok(SystemCall::FdSeek),
            4 => Ok(SystemCall::FdRead),
            5 => Ok(SystemCall::FdFdstatGet),
            6 => Ok(SystemCall::EnvironSizesGet),
            7 => Ok(SystemCall::EnvironGet),
            8 => Ok(SystemCall::FdPrestatGet),
            9 => Ok(SystemCall::FdPrestatDirName),
            10 => Ok(SystemCall::PathOpen),
            11 => Ok(SystemCall::FdFdstatSetFlags),
            12 => Ok(SystemCall::ArgsSizesGet),
            13 => Ok(SystemCall::ArgsGet),
            14 => Ok(SystemCall::ClockTimeGet),
            _ => Err("Unknown WASI system call."),
        }
    }
}

impl From<SystemCall> for usize {
    /// Get system call number from this SystemCall enum.
    ///
    /// # Return
    /// Returns system call number of this SystemCall enum.
    fn from(val: SystemCall) -> Self {
        match val {
            SystemCall::ProcExit => 0,
            SystemCall::FdClose => 1,
            SystemCall::FdWrite => 2,
            SystemCall::FdSeek => 3,
            SystemCall::FdRead => 4,
            SystemCall::FdFdstatGet => 5,
            SystemCall::EnvironSizesGet => 6,
            SystemCall::EnvironGet => 7,
            SystemCall::FdPrestatGet => 8,
            SystemCall::FdPrestatDirName => 9,
            SystemCall::PathOpen => 10,
            SystemCall::FdFdstatSetFlags => 11,
            SystemCall::ArgsSizesGet => 12,
            SystemCall::ArgsGet => 13,
            SystemCall::ClockTimeGet => 14,
        }
    }
}

impl From<SystemCall> for Signature {
    /// Get wasmi function signature of SystemCall enum.
    ///
    /// # Return
    /// Returns wasmi function signature of this SystemCall enum.
    fn from(val: SystemCall) -> Self {
        match val {
            SystemCall::ProcExit => sig!((I32)),
            SystemCall::FdClose => sig!((I32)->I32),
            SystemCall::FdWrite => sig!((I32,I32,I32,I32)->I32),
            SystemCall::FdSeek => sig!((I32,I64,I32,I32)->I32),
            SystemCall::FdRead => sig!((I32,I32,I32,I32)->I32),
            SystemCall::FdFdstatGet => sig!((I32,I32)->I32),
            SystemCall::EnvironSizesGet => sig!((I32,I32)->I32),
            SystemCall::EnvironGet => sig!((I32,I32)->I32),
            SystemCall::FdPrestatGet => sig!((I32,I32)->I32),
            SystemCall::FdPrestatDirName => sig!((I32,I32,I32)->I32),
            SystemCall::PathOpen => sig!((I32,I32,I32,I32,I32,I64,I64,I32,I32)->I32),
            SystemCall::FdFdstatSetFlags => sig!((I32,I32)->I32),
            SystemCall::ArgsSizesGet => sig!((I32,I32)->I32),
            SystemCall::ArgsGet => sig!((I32,I32)->I32),
            SystemCall::ClockTimeGet => sig!((I32,I64,I32)->I32),
        }
    }
}

/// WASI rights of a directory with full permissions.
pub const FULL_DIR_RIGHTS: wasi::Rights = wasi::RIGHTS_FD_FDSTAT_SET_FLAGS
    | wasi::RIGHTS_FD_SYNC
    | wasi::RIGHTS_FD_ADVISE
    | wasi::RIGHTS_PATH_CREATE_DIRECTORY
    | wasi::RIGHTS_PATH_CREATE_FILE
    | wasi::RIGHTS_PATH_LINK_SOURCE
    | wasi::RIGHTS_PATH_LINK_TARGET
    | wasi::RIGHTS_PATH_OPEN
    | wasi::RIGHTS_FD_READDIR
    | wasi::RIGHTS_PATH_READLINK
    | wasi::RIGHTS_PATH_RENAME_SOURCE
    | wasi::RIGHTS_PATH_RENAME_TARGET
    | wasi::RIGHTS_PATH_FILESTAT_GET
    | wasi::RIGHTS_PATH_FILESTAT_SET_SIZE
    | wasi::RIGHTS_PATH_FILESTAT_SET_TIMES
    | wasi::RIGHTS_FD_FILESTAT_GET
    | wasi::RIGHTS_FD_FILESTAT_SET_SIZE
    | wasi::RIGHTS_FD_FILESTAT_SET_TIMES
    | wasi::RIGHTS_PATH_SYMLINK
    | wasi::RIGHTS_PATH_REMOVE_DIRECTORY
    | wasi::RIGHTS_PATH_UNLINK_FILE
    | wasi::RIGHTS_POLL_FD_READWRITE;

/// WASI rights of a file with full permissions.
pub const FULL_FILE_RIGHTS: wasi::Rights = wasi::RIGHTS_FD_DATASYNC
    | wasi::RIGHTS_FD_READ
    | wasi::RIGHTS_FD_SEEK
    | wasi::RIGHTS_FD_FDSTAT_SET_FLAGS
    | wasi::RIGHTS_FD_SYNC
    | wasi::RIGHTS_FD_TELL
    | wasi::RIGHTS_FD_WRITE
    | wasi::RIGHTS_FD_ADVISE
    | wasi::RIGHTS_FD_ALLOCATE
    | wasi::RIGHTS_FD_FILESTAT_GET
    | wasi::RIGHTS_FD_FILESTAT_SET_SIZE
    | wasi::RIGHTS_FD_FILESTAT_SET_TIMES
    | wasi::RIGHTS_POLL_FD_READWRITE
    | wasi::RIGHTS_FD_FILESTAT_SET_TIMES
    | wasi::RIGHTS_POLL_FD_READWRITE;
