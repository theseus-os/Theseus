extern crate alloc;
extern crate terminal_print;
extern crate wasmi;

use crate::wasi_definitions::SystemCall;
use crate::HostExternals;
use alloc::string::String;
use alloc::vec::Vec;
use core::convert::TryFrom as _;
use wasmi::{RuntimeArgs, RuntimeValue, Trap};

pub fn execute_system_call(
    system_call: SystemCall,
    h_ext: &mut HostExternals,
    args: RuntimeArgs,
) -> Result<Option<RuntimeValue>, Trap> {
    match system_call {
        SystemCall::ProcExit => {
            let exit_code: i32 = args.nth_checked(0)?;
            panic!("proc_exit called with {:?}", exit_code);
        }
        SystemCall::FdClose => {
            let fd: i32 = args.nth_checked(0)?;
            return Ok(Some(RuntimeValue::I32(0)));
        }
        SystemCall::FdWrite => {
            let _fd: i32 = args.nth_checked(0)?;
            let addr: i32 = args.nth_checked(1)?;
            let num: i32 = args.nth_checked(2)?;
            let out_ptr: i32 = args.nth_checked(3)?;

            if let Some(ref mut memory) = h_ext.memory {
                let data_to_write = memory.get(addr as u32, 8 * num as usize);
                let mut data_out = Vec::with_capacity(num as usize * 2);

                for elt in data_to_write.unwrap().chunks(4) {
                    data_out.push(u32::from_le_bytes(<[u8; 4]>::try_from(elt).unwrap()));
                }

                let mut written: usize = 0;

                for ptr_and_len in data_out.chunks(2) {
                    let ptr: u32 = ptr_and_len[0];
                    let len: usize = ptr_and_len[1] as usize;

                    let char_arr = memory.get(ptr, len).unwrap();

                    written += len;
                    print!("{}", String::from_utf8(char_arr).unwrap());
                }

                memory
                    .set(out_ptr as u32, &written.to_le_bytes())
                    .expect("memory error");
            }

            return Ok(Some(RuntimeValue::I32(0)));
        }
        SystemCall::FdSeek => {
            let arg1: i32 = args.nth_checked(0)?;
            let arg2: i64 = args.nth_checked(1)?;
            let arg3: i32 = args.nth_checked(2)?;
            let arg4: i32 = args.nth_checked(3)?;
            return Ok(Some(RuntimeValue::I32(0)));
        }
        SystemCall::FdRead => {
            let arg1: i32 = args.nth_checked(0)?;
            let arg2: i32 = args.nth_checked(1)?;
            let arg3: i32 = args.nth_checked(2)?;
            let arg4: i32 = args.nth_checked(3)?;
            return Ok(Some(RuntimeValue::I32(0)));
        }
        SystemCall::FdFdstatGet => {
            let arg1: i32 = args.nth_checked(0)?;
            let arg2: i32 = args.nth_checked(1)?;
            return Ok(Some(RuntimeValue::I32(0)));
        }
        SystemCall::EnvironSizesGet => {
            let arg1: i32 = args.nth_checked(0)?;
            let arg2: i32 = args.nth_checked(1)?;
            return Ok(Some(RuntimeValue::I32(0)));
        }
        SystemCall::EnvironGet => {
            let arg1: i32 = args.nth_checked(0)?;
            let arg2: i32 = args.nth_checked(1)?;
            return Ok(Some(RuntimeValue::I32(0)));
        }
    }
}
