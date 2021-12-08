use crate::posix_file_system::{PosixNode, PosixNodeOrStdio};
use crate::wasi_definitions::SystemCall;
use crate::HostExternals;
use alloc::string::String;
use alloc::vec::Vec;
use core::convert::TryFrom as _;
use fs_node::{DirRef, File, FileOrDir, FileRef, FsNode, WeakDirRef};
use wasmi::{MemoryRef, RuntimeArgs, RuntimeValue, Trap};

fn args_or_env_sizes_get(
    list: &Vec<String>,
    argc_out: u32,
    argv_buf_size_out: u32,
    memory: &MemoryRef,
) -> Result<Option<RuntimeValue>, Trap> {
    let argc: u32 = list.len() as u32;
    let argv_buf_size: u32 = list.iter().fold(0u32, |s, a| {
        s.saturating_add(a.len() as u32).saturating_add(1)
    });

    memory.set(argc_out, &argc.to_le_bytes()).unwrap();
    memory
        .set(argv_buf_size_out, &argv_buf_size.to_le_bytes())
        .unwrap();

    return Ok(Some(RuntimeValue::I32(From::from(wasi::ERRNO_SUCCESS))));
}

fn args_or_env_get(
    list: &Vec<String>,
    argv: u32,
    argv_buf: u32,
    memory: &MemoryRef,
) -> Result<Option<RuntimeValue>, Trap> {
    let mut argv_pos = 0;
    let mut argv_buf_pos = 0;

    for arg in list.iter() {
        let arg = arg.as_bytes();
        memory
            .set(
                argv.checked_add(argv_pos).unwrap(),
                &(argv_buf.checked_add(argv_buf_pos).unwrap()).to_le_bytes(),
            )
            .unwrap();
        argv_pos = argv_pos.checked_add(4).unwrap();
        memory
            .set(argv_buf.checked_add(argv_buf_pos).unwrap(), &arg)
            .unwrap();
        argv_buf_pos = argv_buf_pos.checked_add(arg.len() as u32).unwrap();
        memory
            .set(argv_buf.checked_add(argv_buf_pos).unwrap(), &[0])
            .unwrap();
        argv_buf_pos = argv_buf_pos.checked_add(1).unwrap();
    }

    return Ok(Some(RuntimeValue::I32(From::from(wasi::ERRNO_SUCCESS))));
}

pub fn execute_system_call(
    system_call: SystemCall,
    h_ext: &mut HostExternals,
    wasmi_args: RuntimeArgs,
) -> Result<Option<RuntimeValue>, Trap> {
    let ref mut memory = match h_ext.memory {
        Some(ref mut mem) => mem,
        None => {
            return Ok(Some(RuntimeValue::I32(From::from(wasi::ERRNO_IO))));
        }
    };
    let ref mut fd_table = h_ext.fd_table;
    let ref theseus_env_vars = h_ext.theseus_env_vars;
    let ref theseus_args = h_ext.theseus_args;

    //    println!("Called {:?} with args: {:?}", system_call, wasmi_args);

    match system_call {
        SystemCall::ProcExit => {
            let exit_code: wasi::Exitcode = wasmi_args.nth_checked(0)?;
            h_ext.exit_code = exit_code;
            return Ok(Some(RuntimeValue::I32(From::from(wasi::ERRNO_SUCCESS))));
        }
        SystemCall::FdClose => {
            let fd: wasi::Fd = wasmi_args.nth_checked(0).unwrap();
            match fd_table.close_fd(fd) {
                Ok(_) => Ok(Some(RuntimeValue::I32(From::from(wasi::ERRNO_SUCCESS)))),
                Err(wasi_error) => Ok(Some(RuntimeValue::I32(From::from(wasi_error)))),
            }
        }
        SystemCall::FdWrite => {
            let posix_node_or_stdio: &mut PosixNodeOrStdio = {
                let fd: wasi::Fd = wasmi_args.nth_checked(0).unwrap();
                match fd_table.get_posix_node_or_stdio(fd) {
                    Some(pn) => pn,
                    None => {
                        return Ok(Some(RuntimeValue::I32(From::from(wasi::ERRNO_BADF))));
                    }
                }
            };

            let addr: u32 = wasmi_args.nth_checked(1).unwrap();
            let num: u32 = wasmi_args.nth_checked(2).unwrap();

            let data_to_write = memory.get(addr as u32, 4 * num as usize * 2);
            let mut data_out = Vec::with_capacity(num as usize);

            for elt in data_to_write.unwrap().chunks(4) {
                data_out.push(u32::from_le_bytes(<[u8; 4]>::try_from(elt).unwrap()));
            }

            let mut total_written: usize = 0;

            for ptr_and_len in data_out.chunks(2) {
                let ptr: u32 = ptr_and_len[0];
                let len: usize = ptr_and_len[1] as usize;

                let char_arr = memory.get(ptr, len).unwrap();

                let bytes_written: usize = match posix_node_or_stdio.write(&char_arr) {
                    Ok(bytes_written) => bytes_written,
                    Err(wasi_errno) => {
                        return Ok(Some(RuntimeValue::I32(From::from(wasi_errno))));
                    }
                };
                total_written = total_written.checked_add(bytes_written).unwrap();
            }

            let out_ptr: u32 = wasmi_args.nth_checked(3).unwrap();
            memory.set(out_ptr, &total_written.to_le_bytes()).unwrap();
            return Ok(Some(RuntimeValue::I32(From::from(wasi::ERRNO_SUCCESS))));
        }
        SystemCall::FdSeek => {
            let posix_node_or_stdio: &mut PosixNodeOrStdio = {
                let fd: wasi::Fd = wasmi_args.nth_checked(0).unwrap();
                match fd_table.get_posix_node_or_stdio(fd) {
                    Some(pn) => pn,
                    None => {
                        return Ok(Some(RuntimeValue::I32(From::from(wasi::ERRNO_BADF))));
                    }
                }
            };

            let offset: i64 = wasmi_args.nth_checked(1).unwrap();
            let whence: wasi::Whence = wasmi_args.nth_checked(2).unwrap();

            let new_offset: usize = match posix_node_or_stdio.seek(offset, whence) {
                Ok(new_offset) => new_offset,
                Err(wasi_errno) => {
                    return Ok(Some(RuntimeValue::I32(From::from(wasi_errno))));
                }
            };

            let out_ptr: u32 = wasmi_args.nth_checked(3).unwrap();
            memory.set(out_ptr, &new_offset.to_le_bytes()).unwrap();
            return Ok(Some(RuntimeValue::I32(From::from(wasi::ERRNO_SUCCESS))));
        }
        SystemCall::FdRead => {
            let posix_node_or_stdio: &mut PosixNodeOrStdio = {
                let fd: wasi::Fd = wasmi_args.nth_checked(0).unwrap();
                match fd_table.get_posix_node_or_stdio(fd) {
                    Some(pn) => pn,
                    None => {
                        return Ok(Some(RuntimeValue::I32(From::from(wasi::ERRNO_BADF))));
                    }
                }
            };

            let addr: u32 = wasmi_args.nth_checked(1).unwrap();
            let num: u32 = wasmi_args.nth_checked(2).unwrap();

            let list_buf = memory.get(addr as u32, 8 * num as usize);
            let mut out_buffers_list = Vec::with_capacity(num as usize);

            for elt in list_buf.unwrap().chunks(4) {
                out_buffers_list.push(u32::from_le_bytes(<[u8; 4]>::try_from(elt).unwrap()));
            }

            let mut total_read: usize = 0;

            for ptr_and_len in out_buffers_list.chunks(2) {
                let ptr: u32 = ptr_and_len[0];
                let len: usize = ptr_and_len[1] as usize;

                let ref mut read_buf = vec![0; len];

                let bytes_read: usize = match posix_node_or_stdio.read(read_buf) {
                    Ok(bytes_written) => bytes_written,
                    Err(wasi_errno) => {
                        return Ok(Some(RuntimeValue::I32(From::from(wasi_errno))));
                    }
                };

                //println!("{}", String::from_utf8_lossy(&read_buf[0..bytes_read]));
                memory.set(ptr, &read_buf[0..bytes_read]).unwrap();
                total_read = total_read.checked_add(bytes_read).unwrap();
            }

            let out_ptr: u32 = wasmi_args.nth_checked(3).unwrap();
            memory.set(out_ptr, &total_read.to_le_bytes()).unwrap();
            return Ok(Some(RuntimeValue::I32(From::from(wasi::ERRNO_SUCCESS))));
        }
        SystemCall::FdFdstatGet => {
            let posix_node: &mut PosixNode = {
                let fd: wasi::Fd = wasmi_args.nth_checked(0).unwrap();
                match fd_table.get_posix_node(fd) {
                    Some(pn) => pn,
                    None => {
                        return Ok(Some(RuntimeValue::I32(From::from(wasi::ERRNO_BADF))));
                    }
                }
            };

            let stat = match posix_node.theseus_file_or_dir() {
                FileOrDir::Dir { .. } => wasi::Fdstat {
                    fs_filetype: wasi::FILETYPE_DIRECTORY,
                    fs_flags: posix_node.fd_flags(),
                    fs_rights_base: posix_node.fs_rights_base(),
                    fs_rights_inheriting: posix_node.fs_rights_inheriting(),
                },
                FileOrDir::File { .. } => wasi::Fdstat {
                    fs_filetype: wasi::FILETYPE_REGULAR_FILE,
                    fs_flags: posix_node.fd_flags(),
                    fs_rights_base: posix_node.fs_rights_base(),
                    fs_rights_inheriting: posix_node.fs_rights_inheriting(),
                },
            };

            let stat_buf: u32 = wasmi_args.nth_checked(1).unwrap();

            memory.set(stat_buf, &[0; 24]).unwrap();
            memory.set(stat_buf, &[stat.fs_filetype]).unwrap();
            memory
                .set(
                    stat_buf.checked_add(2).unwrap(),
                    &stat.fs_flags.to_le_bytes(),
                )
                .unwrap();
            memory
                .set(
                    stat_buf.checked_add(8).unwrap(),
                    &stat.fs_rights_base.to_le_bytes(),
                )
                .unwrap();
            memory
                .set(
                    stat_buf.checked_add(16).unwrap(),
                    &stat.fs_rights_inheriting.to_le_bytes(),
                )
                .unwrap();

            return Ok(Some(RuntimeValue::I32(From::from(wasi::ERRNO_SUCCESS))));
        }
        SystemCall::EnvironSizesGet => {
            let envc_out: u32 = wasmi_args.nth_checked(0).unwrap();
            let envv_buf_size_out: u32 = wasmi_args.nth_checked(1).unwrap();
            args_or_env_sizes_get(theseus_env_vars, envc_out, envv_buf_size_out, memory)
        }
        SystemCall::EnvironGet => {
            let envv: u32 = wasmi_args.nth_checked(0).unwrap();
            let envv_buf: u32 = wasmi_args.nth_checked(1).unwrap();
            args_or_env_get(theseus_env_vars, envv, envv_buf, memory)
        }
        SystemCall::FdPrestatGet => {
            let posix_node: &mut PosixNode = {
                let fd: wasi::Fd = wasmi_args.nth_checked(0).unwrap();
                match fd_table.get_posix_node(fd) {
                    Some(pn) => pn,
                    None => {
                        return Ok(Some(RuntimeValue::I32(From::from(wasi::ERRNO_BADF))));
                    }
                }
            };

            let pr_name_len: u32 = match posix_node.theseus_file_or_dir() {
                FileOrDir::File { .. } => {
                    return Ok(Some(RuntimeValue::I32(From::from(wasi::ERRNO_NOTSUP))));
                }
                FileOrDir::Dir { .. } => {
                    posix_node.theseus_file_or_dir().get_name().chars().count() as u32
                }
            };

            let prestat_buf: u32 = wasmi_args.nth_checked(1).unwrap();
            memory.set(prestat_buf, &[0; 8]).unwrap();
            memory.set(prestat_buf, &[wasi::PREOPENTYPE_DIR]).unwrap();
            memory
                .set(
                    prestat_buf.checked_add(4).unwrap(),
                    &pr_name_len.to_le_bytes(),
                )
                .unwrap();

            return Ok(Some(RuntimeValue::I32(From::from(wasi::ERRNO_SUCCESS))));
        }
        SystemCall::FdPrestatDirName => {
            let posix_node: &mut PosixNode = {
                let fd: wasi::Fd = wasmi_args.nth_checked(0).unwrap();
                match fd_table.get_posix_node(fd) {
                    Some(pn) => pn,
                    None => {
                        return Ok(Some(RuntimeValue::I32(From::from(wasi::ERRNO_BADF))));
                    }
                }
            };

            let name = match posix_node.theseus_file_or_dir() {
                FileOrDir::File { .. } => {
                    return Ok(Some(RuntimeValue::I32(From::from(wasi::ERRNO_NOTSUP))));
                }
                FileOrDir::Dir { .. } => posix_node.theseus_file_or_dir().get_name(),
            };

            let path_out_buf: u32 = wasmi_args.nth_checked(1).unwrap();
            let path_out_len: u32 = wasmi_args.nth_checked(2).unwrap();

            memory
                .set(path_out_buf, &name.as_bytes()[..path_out_len as usize])
                .unwrap();

            return Ok(Some(RuntimeValue::I32(From::from(wasi::ERRNO_SUCCESS))));
        }
        SystemCall::PathOpen => {
            let posix_node: &mut PosixNode = {
                let fd: wasi::Fd = wasmi_args.nth_checked(0).unwrap();
                match fd_table.get_posix_node(fd) {
                    Some(pn) => pn,
                    None => {
                        return Ok(Some(RuntimeValue::I32(From::from(wasi::ERRNO_BADF))));
                    }
                }
            };

            let parent_dir: DirRef = match posix_node.theseus_file_or_dir().clone() {
                FileOrDir::File { .. } => {
                    return Ok(Some(RuntimeValue::I32(From::from(wasi::ERRNO_NOTDIR))));
                }
                FileOrDir::Dir(dir_ref) => dir_ref.clone(),
            };

            let lookup_flags: wasi::Lookupflags = wasmi_args.nth_checked(1).unwrap();

            let path = {
                let path_buf: u32 = wasmi_args.nth_checked(2).unwrap();
                let path_buf_len: u32 = wasmi_args.nth_checked(3).unwrap();
                let path_utf8 = memory.get(path_buf, path_buf_len as usize).unwrap();
                String::from_utf8(path_utf8).unwrap()
            };

            let open_flags: wasi::Oflags = wasmi_args.nth_checked(4).unwrap();
            let fs_rights_base: wasi::Rights = wasmi_args.nth_checked(5).unwrap();
            let fs_rights_inheriting: wasi::Rights = wasmi_args.nth_checked(6).unwrap();
            let fd_flags: wasi::Fdflags = wasmi_args.nth_checked(7).unwrap();

            let opened_fd: wasi::Fd = match fd_table.open_path(
                &path,
                parent_dir,
                lookup_flags,
                open_flags,
                fs_rights_base,
                fs_rights_inheriting,
                fd_flags,
            ) {
                Ok(fd) => fd,
                Err(wasi_error) => {
                    return Ok(Some(RuntimeValue::I32(From::from(wasi_error))));
                }
            };

            let opened_fd_ptr: u32 = wasmi_args.nth_checked(8).unwrap();
            memory.set(opened_fd_ptr, &opened_fd.to_le_bytes()).unwrap();
            return Ok(Some(RuntimeValue::I32(From::from(wasi::ERRNO_SUCCESS))));
        }
        SystemCall::FdFdstatSetFlags => {
            panic!("unimplemented {:?}", system_call);
            return Ok(Some(RuntimeValue::I32(From::from(wasi::ERRNO_SUCCESS))));
        }
        SystemCall::ArgsSizesGet => {
            let argc_out: u32 = wasmi_args.nth_checked(0).unwrap();
            let argv_buf_size_out: u32 = wasmi_args.nth_checked(1).unwrap();
            args_or_env_sizes_get(theseus_args, argc_out, argv_buf_size_out, memory)
        }
        SystemCall::ArgsGet => {
            let argv: u32 = wasmi_args.nth_checked(0).unwrap();
            let argv_buf: u32 = wasmi_args.nth_checked(1).unwrap();
            args_or_env_get(theseus_args, argv, argv_buf, memory)
        }
        SystemCall::ClockTimeGet => {
            // TODO Actually implement
            return Ok(Some(RuntimeValue::I32(From::from(wasi::ERRNO_SUCCESS))));
        }
    }
}
