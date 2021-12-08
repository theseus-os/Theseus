use alloc::vec::Vec;
use smallvec::SmallVec;
use wasmi::{Signature, ValueType};

pub fn get_signature(
    params: impl Iterator<Item = ValueType>,
    ret_ty: impl Into<Option<ValueType>>,
) -> Signature {
    let params: SmallVec<[ValueType; 2]> = params.collect();
    let ret_ty: Option<ValueType> = ret_ty.into();

    wasmi::Signature::new(
        params
            .iter()
            .cloned()
            .map(wasmi::ValueType::from)
            .collect::<Vec<_>>(),
        ret_ty.map(wasmi::ValueType::from),
    )
}

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

// TODO find macros to cut down on duplicated logic
#[derive(Clone, Copy, Debug)]
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

impl SystemCall {
    pub fn from_usize(syscall_index: usize) -> Result<SystemCall, ()> {
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
            _ => Err(()),
        }
    }

    pub fn from_fn_name(fn_name: &str) -> Result<SystemCall, ()> {
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
            _ => Err(()),
        }
    }

    pub fn to_usize(&self) -> usize {
        match self {
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

    pub fn signature(&self) -> Signature {
        match self {
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
