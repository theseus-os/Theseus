extern crate alloc;
extern crate wasmi;

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

fn signature_type_to_signature(signature_type: usize) -> Signature {
    match signature_type {
        0 => sig!((I32)->I32),
        2 => sig!((I32,I32)->I32),
        8 => sig!((I32)),
        9 => sig!((I32,I32,I32,I32)->I32),
        16 => sig!((I32,I64, I32,I32)->I32),
        _ => panic!("Missing signature type: {}", signature_type),
    }
}

#[derive(Clone, Copy)]
pub enum SystemCall {
    ProcExit,
    FdClose,
    FdWrite,
    FdSeek,
    FdRead,
    FdFdstatGet,
    EnvironSizesGet,
    EnvironGet,
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
        }
    }

    pub fn signature(&self) -> Signature {
        signature_type_to_signature(self.to_signature_type())
    }

    fn to_signature_type(&self) -> usize {
        match self {
            SystemCall::ProcExit => 8,
            SystemCall::FdClose => 0,
            SystemCall::FdWrite => 9,
            SystemCall::FdSeek => 16,
            SystemCall::FdRead => 9,
            SystemCall::FdFdstatGet => 2,
            SystemCall::EnvironSizesGet => 2,
            SystemCall::EnvironGet => 2,
        }
    }
}
