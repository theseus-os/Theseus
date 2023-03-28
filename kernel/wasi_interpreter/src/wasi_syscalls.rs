//! WASI system call implementations.
//!
//! This module implements system calls required by WASI. These system calls interact with wasmi
//! memory and Theseus I/O (standard I/O, file system, arguments, environment variables, etc.).
//!
//! Documentation on system call behavior:
//! <https://docs.rs/wasi/0.10.2+wasi-snapshot-preview1/wasi/index.html>
//! <https://github.com/WebAssembly/wasi-libc/blob/ad5133410f66b93a2381db5b542aad5e0964db96/libc-bottom-half/headers/public/wasi/api.h>
//!
//! The WASI crate documentation is useful for understanding WASI crate types and for a high-level
//! understanding of WASI standards. The wasi-lib.c API header is far more useful, however, for
//! understanding implementation details such as exact arguments passed into WASI system calls.
//!
//! Inspiration for some implementations is borrowed from tomaka/redshirt:
//! <https://github.com/tomaka/redshirt/blob/4df506f68821353a7fd67bb94c4223df6b683e1b/kernel/core/src/extrinsics/wasi.rs>
//!

use crate::posix_file_system::{PosixNode, PosixNodeOrStdio};
use crate::wasi_definitions::SystemCall;
use crate::HostExternals;
use alloc::string::String;
use alloc::vec::Vec;
use core::convert::TryFrom as _;
use fs_node::{DirRef, FileOrDir};
use wasmi::{MemoryRef, RuntimeArgs, RuntimeValue, Trap};

/// Helper function to support retrieving args/env sizes.
///
/// # Arguments
/// * `list`: string vector of which size is being retrieved (args or env).
/// * `argc_out`: pointer to store length of list.
/// * `argv_buf_size_out`: pointer to store total length of data in list. This includes null
/// terminating bytes used by strings.
/// * `memory`: wasmi memory buffer.
///
/// # Return
/// A WASI errno.
fn args_or_env_sizes_get(
    list: &Vec<String>,
    argc_out: u32,
    argv_buf_size_out: u32,
    memory: &MemoryRef,
) -> Result<Option<RuntimeValue>, Trap> {
    // Compute length of list.
    let argc: wasi::Size = list.len();
    // Compute length of data within list.
    let argv_buf_size: wasi::Size = list
        .iter()
        .fold(0, |s, a| s.saturating_add(a.len()).saturating_add(1));

    // Write lengths to memory
    memory.set(argc_out, &argc.to_le_bytes()).unwrap();
    memory
        .set(argv_buf_size_out, &argv_buf_size.to_le_bytes())
        .unwrap();

    Ok(Some(RuntimeValue::I32(From::from(wasi::ERRNO_SUCCESS))))
}

/// Helper function to support retrieving args/env data.
///
/// # Arguments
/// * `list`: string vector of which data is being retrieved (args or env).
/// * `argv_pointers`: buffer to store pointers to each element in list.
/// * `argv_data`: buffer to store data in list. This includes null terminating bytes for each
/// element in list.
/// * `memory`: wasmi memory buffer.
///
/// # Return
/// A WASI errno.
fn args_or_env_get(
    list: &[String],
    argv_pointers: u32,
    argv_data: u32,
    memory: &MemoryRef,
) -> Result<Option<RuntimeValue>, Trap> {
    let mut argv_pointers_pos: u32 = 0;
    let mut argv_data_pos: u32 = 0;

    for arg in list.iter() {
        let arg = arg.as_bytes();

        // Write pointer to current arg to argv_pointers buffer.
        memory
            .set(
                argv_pointers.checked_add(argv_pointers_pos).unwrap(),
                &(argv_data.checked_add(argv_data_pos).unwrap()).to_le_bytes(),
            )
            .unwrap();
        argv_pointers_pos = argv_pointers_pos.checked_add(4).unwrap();

        // Write content of current arg to argv_data buffer.
        memory
            .set(argv_data.checked_add(argv_data_pos).unwrap(), arg)
            .unwrap();
        argv_data_pos = argv_data_pos
            .checked_add(u32::try_from(arg.len()).unwrap())
            .unwrap();

        // Write null terminating byte to argv_data buffer.
        memory
            .set(argv_data.checked_add(argv_data_pos).unwrap(), &[0])
            .unwrap();
        argv_data_pos = argv_data_pos.checked_add(1).unwrap();
    }

    Ok(Some(RuntimeValue::I32(From::from(wasi::ERRNO_SUCCESS))))
}

/// Executes specified system calls by interacting with wasmi memory and Theseus I/O.
///
/// # Arguments
/// * `system_call`: SystemCall enum specifying which system call to execute.
/// * `h_ext`: object containing wasmi memory and Theseus I/O.
/// * `wasmi_args`: system call arguments.
///
/// # Return
/// Returns result of wasmi RuntimeValue on success or wasmi Trap on error used by wasmi to
/// continue binary execution.
pub fn execute_system_call(
    system_call: SystemCall,
    h_ext: &mut HostExternals,
    wasmi_args: RuntimeArgs,
) -> Result<Option<RuntimeValue>, Trap> {
    // Handles to wasmi memory and Theseus I/O.
    let memory = &mut match h_ext.memory {
        Some(ref mut mem) => mem,
        None => {
            return Ok(Some(RuntimeValue::I32(From::from(wasi::ERRNO_NOMEM))));
        }
    };
    let fd_table = &mut h_ext.fd_table;
    let theseus_env_vars = &h_ext.theseus_env_vars;
    let theseus_args = &h_ext.theseus_args;

    match system_call {
        // Terminate process with given exit code.
        //
        // # Arguments
        // * `exit_code`: the exit code being returned by the process.
        //
        // # Return
        // None.
        SystemCall::ProcExit => {
            let exit_code: wasi::Exitcode = wasmi_args.nth_checked(0)?;
            h_ext.exit_code = exit_code;
            Ok(None)
        }

        // Close a file descriptor. This is similar to close in POSIX.
        //
        // # Arguments
        // * `fd`: the file descriptor number to close.
        //
        // # Return
        // A WASI errno.
        SystemCall::FdClose => {
            let fd: wasi::Fd = wasmi_args.nth_checked(0).unwrap();
            match fd_table.close_fd(fd) {
                Ok(_) => Ok(Some(RuntimeValue::I32(From::from(wasi::ERRNO_SUCCESS)))),
                Err(wasi_error) => Ok(Some(RuntimeValue::I32(From::from(wasi_error)))),
            }
        }

        // Write to a file descriptor. This is similar to writev in POSIX.
        //
        // # Arguments
        // * `fd`: the file descriptor number to write to.
        // * `iovs`: pointer to list of scatter/gather vectors from which to retrieve data. each
        // element in list consists of a 32 bit pointer to a data vector and a 32 bit length
        // associated with that data vector (8 bytes).
        // * `iovs_len`: the length of the array pointed to by `iovs`.
        // * `ret_ptr`: pointer to write number of bytes written to.
        //
        // # Return
        // A WASI errno.
        SystemCall::FdWrite => {
            // Fetch file to write to.
            let posix_node_or_stdio: &mut PosixNodeOrStdio = {
                let fd: wasi::Fd = wasmi_args.nth_checked(0).unwrap();
                match fd_table.get_posix_node_or_stdio(fd) {
                    Some(pn) => pn,
                    None => {
                        return Ok(Some(RuntimeValue::I32(From::from(wasi::ERRNO_BADF))));
                    }
                }
            };

            let iovs: u32 = wasmi_args.nth_checked(1).unwrap();
            let iovs_len: wasi::Size = {
                let len: u32 = wasmi_args.nth_checked(2).unwrap();
                wasi::Size::try_from(len).unwrap()
            };

            // convert iovs into vector of (ptr,len) pairs.
            let data_pointers = memory.get(iovs, 4 * iovs_len * 2);
            let mut data_out = Vec::with_capacity(iovs_len * 2);

            for elt in data_pointers.unwrap().chunks(4) {
                data_out.push(u32::from_le_bytes(<[u8; 4]>::try_from(elt).unwrap()));
            }

            let mut total_written: usize = 0;

            for ptr_and_len in data_out.chunks(2) {
                let ptr: u32 = ptr_and_len[0];
                let len: wasi::Size = wasi::Size::try_from(ptr_and_len[1]).unwrap();

                // retrieve data to write.
                let char_arr = memory.get(ptr, len).unwrap();

                let bytes_written: wasi::Size = match posix_node_or_stdio.write(&char_arr) {
                    Ok(bytes_written) => bytes_written,
                    Err(wasi_errno) => {
                        return Ok(Some(RuntimeValue::I32(From::from(wasi_errno))));
                    }
                };
                total_written = total_written.checked_add(bytes_written).unwrap();
            }

            // write bytes written to return pointer.
            let ret_ptr: u32 = wasmi_args.nth_checked(3).unwrap();
            memory.set(ret_ptr, &total_written.to_le_bytes()).unwrap();
            Ok(Some(RuntimeValue::I32(From::from(wasi::ERRNO_SUCCESS))))
        }

        // Move the offset of a file descriptor. This is similar to lseek in POSIX.
        //
        // # Arguments
        // * `fd`: the file descriptor number to seek.
        // * `offset`: the number of bytes to move.
        // * `whence`: the base from which the offset is relative.
        // * `ret_ptr`: pointer to write new offset to.
        //
        // # Return
        // A WASI errno.
        SystemCall::FdSeek => {
            // fetch file to seek.
            let posix_node_or_stdio: &mut PosixNodeOrStdio = {
                let fd: wasi::Fd = wasmi_args.nth_checked(0).unwrap();
                match fd_table.get_posix_node_or_stdio(fd) {
                    Some(pn) => pn,
                    None => {
                        return Ok(Some(RuntimeValue::I32(From::from(wasi::ERRNO_BADF))));
                    }
                }
            };

            let offset: wasi::Filedelta = wasmi_args.nth_checked(1).unwrap();
            let whence: wasi::Whence = wasmi_args.nth_checked(2).unwrap();

            // move offset.
            let new_offset: wasi::Filesize = match posix_node_or_stdio.seek(offset, whence) {
                Ok(new_offset) => wasi::Filesize::try_from(new_offset).unwrap(),
                Err(wasi_errno) => {
                    return Ok(Some(RuntimeValue::I32(From::from(wasi_errno))));
                }
            };

            // write new offset to return pointer.
            let ret_ptr: u32 = wasmi_args.nth_checked(3).unwrap();
            memory.set(ret_ptr, &new_offset.to_le_bytes()).unwrap();
            Ok(Some(RuntimeValue::I32(From::from(wasi::ERRNO_SUCCESS))))
        }

        // Read from a file descriptor. This is similar to readv in POSIX.
        //
        // # Arguments
        // * `fd`: the file descriptor number to read from.
        // * `iovs`: pointer to list of scatter/gather vectors to which to store data. each element
        // in list consists of a 32 bit pointer to a data vector and a 32 bit length associated with
        // that data vector (8 bytes).
        // * `iovs_len`: the length of the array pointed to by `iovs`.
        // * `ret_ptr`: pointer to write number of bytes read from.
        //
        // # Return
        // A WASI errno.
        SystemCall::FdRead => {
            // fetch file to read from.
            let posix_node_or_stdio: &mut PosixNodeOrStdio = {
                let fd: wasi::Fd = wasmi_args.nth_checked(0).unwrap();
                match fd_table.get_posix_node_or_stdio(fd) {
                    Some(pn) => pn,
                    None => {
                        return Ok(Some(RuntimeValue::I32(From::from(wasi::ERRNO_BADF))));
                    }
                }
            };

            let iovs: u32 = wasmi_args.nth_checked(1).unwrap();
            let iovs_len: wasi::Size = {
                let len: u32 = wasmi_args.nth_checked(2).unwrap();
                wasi::Size::try_from(len).unwrap()
            };

            // convert iovs into vector of (ptr,len) pairs.
            let input_pointers = memory.get(iovs, 4 * iovs_len * 2);
            let mut data_in = Vec::with_capacity(iovs_len * 2);

            for elt in input_pointers.unwrap().chunks(4) {
                data_in.push(u32::from_le_bytes(<[u8; 4]>::try_from(elt).unwrap()));
            }

            let mut total_read: wasi::Size = 0;

            for ptr_and_len in data_in.chunks(2) {
                let ptr: u32 = ptr_and_len[0];
                let len: wasi::Size = wasi::Size::try_from(ptr_and_len[1]).unwrap();

                let read_buf = &mut vec![0; len];

                // retrieve data to read.
                let bytes_read: wasi::Size = match posix_node_or_stdio.read(read_buf) {
                    Ok(bytes_written) => bytes_written,
                    Err(wasi_errno) => {
                        return Ok(Some(RuntimeValue::I32(From::from(wasi_errno))));
                    }
                };

                memory.set(ptr, &read_buf[0..bytes_read]).unwrap();
                total_read = total_read.checked_add(bytes_read).unwrap();
            }

            // write bytes read to return pointer.
            let ret_ptr: u32 = wasmi_args.nth_checked(3).unwrap();
            memory.set(ret_ptr, &total_read.to_le_bytes()).unwrap();
            Ok(Some(RuntimeValue::I32(From::from(wasi::ERRNO_SUCCESS))))
        }

        // Get the attributes of a file descriptor.
        // This returns similar flags to fsync(fd, F_GETFL) in POSIX.
        //
        // # Arguments
        // * `fd`: the file descriptor number to get attributes of.
        // * `stat_buf`: the buffer to store file descriptor's attributes.
        //
        // # Return
        // A WASI errno.
        SystemCall::FdFdstatGet => {
            // fetch file system node to retrieve attributes from.
            let posix_node: &mut PosixNode = {
                let fd: wasi::Fd = wasmi_args.nth_checked(0).unwrap();
                match fd_table.get_posix_node(fd) {
                    Some(pn) => pn,
                    None => {
                        return Ok(Some(RuntimeValue::I32(From::from(wasi::ERRNO_BADF))));
                    }
                }
            };

            // fetch attributes.
            let stat: wasi::Fdstat = match posix_node.theseus_file_or_dir {
                FileOrDir::Dir { .. } => wasi::Fdstat {
                    fs_filetype: wasi::FILETYPE_DIRECTORY,
                    fs_flags: posix_node.fs_flags(),
                    fs_rights_base: posix_node.fs_rights_base(),
                    fs_rights_inheriting: posix_node.fs_rights_inheriting(),
                },
                FileOrDir::File { .. } => wasi::Fdstat {
                    fs_filetype: wasi::FILETYPE_REGULAR_FILE,
                    fs_flags: posix_node.fs_flags(),
                    fs_rights_base: posix_node.fs_rights_base(),
                    fs_rights_inheriting: posix_node.fs_rights_inheriting(),
                },
            };

            let stat_buf: u32 = wasmi_args.nth_checked(1).unwrap();

            // write attributes to stat_buf.
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

            Ok(Some(RuntimeValue::I32(From::from(wasi::ERRNO_SUCCESS))))
        }

        // Return environment variable data sizes.
        //
        // # Arguments
        // * `envc_ptr`: the buffer to store the number of environment variables.
        // * `envv_buf_size_ptr`: the buffer to store the total length of environment variables.
        //
        // # Return
        // A WASI errno.
        SystemCall::EnvironSizesGet => {
            let envc_ptr: u32 = wasmi_args.nth_checked(0).unwrap();
            let envv_buf_size_ptr: u32 = wasmi_args.nth_checked(1).unwrap();
            args_or_env_sizes_get(theseus_env_vars, envc_ptr, envv_buf_size_ptr, memory)
        }

        // Return environment variable data.
        //
        // # Arguments
        // * `envv_pointers`: the buffer to store pointers to each environment variable.
        // * `envv_data`: the buffer to store environment variables.
        //
        // # Return
        // A WASI errno.
        SystemCall::EnvironGet => {
            let envv_pointers: u32 = wasmi_args.nth_checked(0).unwrap();
            let envv_data: u32 = wasmi_args.nth_checked(1).unwrap();
            args_or_env_get(theseus_env_vars, envv_pointers, envv_data, memory)
        }

        // Return a description of the given preopened file descriptor.
        //
        // The preopened descriptor consists of a wasi::Preopentype and the length of the underlying
        // file name (the relative file path).
        //
        // # Arguments
        // * `fd`: the file descriptor number to get preopened description of.
        // * `ret_ptr`: the pointer to write preopened file descriptor.
        //
        // # Return
        // A WASI errno.
        SystemCall::FdPrestatGet => {
            // fetch file to retrieve attributes from.
            let posix_node: &mut PosixNode = {
                let fd: wasi::Fd = wasmi_args.nth_checked(0).unwrap();
                match fd_table.get_posix_node(fd) {
                    Some(pn) => pn,
                    None => {
                        return Ok(Some(RuntimeValue::I32(From::from(wasi::ERRNO_BADF))));
                    }
                }
            };

            // get directory name (relative path).
            let pr_name_len: u32 = match posix_node.theseus_file_or_dir {
                FileOrDir::File { .. } => {
                    return Ok(Some(RuntimeValue::I32(From::from(wasi::ERRNO_NOTDIR))));
                }
                FileOrDir::Dir { .. } => {
                    u32::try_from(posix_node.get_relative_path().chars().count()).unwrap()
                }
            };

            let ret_ptr: u32 = wasmi_args.nth_checked(1).unwrap();

            // write preopened file descriptor to ret_ptr.
            // wasi::PREOPENTYPE_DIR is of type u8 but memory is aligned in sets of 4 bytes.
            memory.set(ret_ptr, &[0; 8]).unwrap();
            memory.set(ret_ptr, &[wasi::PREOPENTYPE_DIR]).unwrap();
            memory
                .set(ret_ptr.checked_add(4).unwrap(), &pr_name_len.to_le_bytes())
                .unwrap();

            Ok(Some(RuntimeValue::I32(From::from(wasi::ERRNO_SUCCESS))))
        }

        // Return the directory name of the given preopened file descriptor.
        //
        // # Arguments
        // * `fd`: the file descriptor number to get name of.
        // * `path`: the buffer into which to write the preopened directory name.
        // * `path_len`: the length of the preopened directory name.
        //
        // # Return
        // A WASI errno.
        SystemCall::FdPrestatDirName => {
            // fetch file to retrieve name of.
            let posix_node: &mut PosixNode = {
                let fd: wasi::Fd = wasmi_args.nth_checked(0).unwrap();
                match fd_table.get_posix_node(fd) {
                    Some(pn) => pn,
                    None => {
                        return Ok(Some(RuntimeValue::I32(From::from(wasi::ERRNO_BADF))));
                    }
                }
            };

            // get directory name (relative path).
            let name = match posix_node.theseus_file_or_dir {
                FileOrDir::File { .. } => {
                    return Ok(Some(RuntimeValue::I32(From::from(wasi::ERRNO_NOTDIR))));
                }
                FileOrDir::Dir { .. } => posix_node.get_relative_path(),
            };

            let path: u32 = wasmi_args.nth_checked(1).unwrap();
            let path_out_len: wasi::Size = {
                let len: u32 = wasmi_args.nth_checked(2).unwrap();
                wasi::Size::try_from(len).unwrap()
            };

            // write directory name to path.
            memory.set(path, &name.as_bytes()[..path_out_len]).unwrap();

            Ok(Some(RuntimeValue::I32(From::from(wasi::ERRNO_SUCCESS))))
        }

        // Open file or directory. This is similar to openat in POSIX.
        //
        // # Arguments
        // * `fd`: the file descriptor number to open from.
        // * `dirflags`: flags determining the method of how the path is resolved.
        // * `path`: the relative path of the file or directory to open, relative to fd directory.
        // * `path_len`: length of path buffer containing path to open.
        // * `open_flags`: the method by which to open the file. this includes modes such as
        // whether to create a file if it doesn't exist, truncate existing files, etc.
        // * `fs_rights_base`: rights applying to the file being opened. this includes rights such
        // as whether reading, writing, setting flags, etc. are permitted.
        // * `fs_rights_inheriting`: rights inherited by files opened by the resulting opened file.
        // * `fs_flags`: file descriptor flags. this includes modes such as append mode when
        // writign to files.
        // * `ret_ptr`: the pointer to write the file descriptor of the opened file or directory.
        //
        // # Return
        // A WASI errno.
        SystemCall::PathOpen => {
            // fetch file system node to open from.
            let posix_node: &mut PosixNode = {
                let fd: wasi::Fd = wasmi_args.nth_checked(0).unwrap();
                match fd_table.get_posix_node(fd) {
                    Some(pn) => pn,
                    None => {
                        return Ok(Some(RuntimeValue::I32(From::from(wasi::ERRNO_BADF))));
                    }
                }
            };

            // verify that file descriptor has rights to open path from.
            if posix_node.fs_rights_base() & wasi::RIGHTS_PATH_OPEN == 0 {
                return Ok(Some(RuntimeValue::I32(From::from(wasi::ERRNO_ACCES))));
            }

            // fetch underlying directory.
            let parent_dir: DirRef = match posix_node.theseus_file_or_dir.clone() {
                FileOrDir::File { .. } => {
                    return Ok(Some(RuntimeValue::I32(From::from(wasi::ERRNO_NOTDIR))));
                }
                FileOrDir::Dir(dir_ref) => dir_ref,
            };

            let lookup_flags: wasi::Lookupflags = wasmi_args.nth_checked(1).unwrap();

            // access path to open.
            let path = {
                let path: u32 = wasmi_args.nth_checked(2).unwrap();
                let path_len: wasi::Size = {
                    let len: u32 = wasmi_args.nth_checked(3).unwrap();
                    wasi::Size::try_from(len).unwrap()
                };
                let path_utf8 = memory.get(path, path_len).unwrap();
                String::from_utf8(path_utf8).unwrap()
            };

            let maximum_rights: wasi::Rights = posix_node.fs_rights_inheriting();

            let open_flags: wasi::Oflags = wasmi_args.nth_checked(4).unwrap();
            let mut fs_rights_base: wasi::Rights = wasmi_args.nth_checked(5).unwrap();
            let mut fs_rights_inheriting: wasi::Rights = wasmi_args.nth_checked(6).unwrap();
            let fs_flags: wasi::Fdflags = wasmi_args.nth_checked(7).unwrap();

            // set rights from parent file descriptor's inheriting rights.
            fs_rights_base &= maximum_rights;
            fs_rights_inheriting &= maximum_rights;

            // open path from file descriptor.
            let opened_fd: wasi::Fd = match fd_table.open_path(
                &path,
                parent_dir,
                lookup_flags,
                open_flags,
                fs_rights_base,
                fs_rights_inheriting,
                fs_flags,
            ) {
                Ok(fd) => fd,
                Err(wasi_error) => {
                    return Ok(Some(RuntimeValue::I32(From::from(wasi_error))));
                }
            };

            // write opened file descriptor to return pointer.
            let opened_fd_ptr: u32 = wasmi_args.nth_checked(8).unwrap();
            memory.set(opened_fd_ptr, &opened_fd.to_le_bytes()).unwrap();
            Ok(Some(RuntimeValue::I32(From::from(wasi::ERRNO_SUCCESS))))
        }

        // Adjust the flags associated with a file descriptor.
        // This is similar to fcntl(fd, F_SETFL, flags) in POSIX.
        //
        // # Arguments
        // * `fd`: the file descriptor being modified.
        // * `flags`: the desired values of the file descriptor flags.
        //
        // # Return
        // A WASI errno.
        SystemCall::FdFdstatSetFlags => {
            // fetch file system node to modify flags.
            let posix_node: &mut PosixNode = {
                let fd: wasi::Fd = wasmi_args.nth_checked(0).unwrap();
                match fd_table.get_posix_node(fd) {
                    Some(pn) => pn,
                    None => {
                        return Ok(Some(RuntimeValue::I32(From::from(wasi::ERRNO_BADF))));
                    }
                }
            };

            // set flags of file.
            let flags: wasi::Fdflags = wasmi_args.nth_checked(1).unwrap();
            match posix_node.set_fs_flags(flags) {
                Ok(_) => {}
                Err(wasi_error) => {
                    return Ok(Some(RuntimeValue::I32(From::from(wasi_error))));
                }
            };

            Ok(Some(RuntimeValue::I32(From::from(wasi::ERRNO_SUCCESS))))
        }

        // Return argument data sizes.
        //
        // # Arguments
        // * `argc_ptr`: the buffer to store the number of arguments.
        // * `argv_buf_size_ptr`: the buffer to store the total length of arguments data.
        //
        // # Return
        // A WASI errno.
        SystemCall::ArgsSizesGet => {
            let argc_ptr: u32 = wasmi_args.nth_checked(0).unwrap();
            let argv_buf_size_ptr: u32 = wasmi_args.nth_checked(1).unwrap();
            args_or_env_sizes_get(theseus_args, argc_ptr, argv_buf_size_ptr, memory)
        }

        // Return arguments.
        //
        // # Arguments
        // * `argv_pointers`: the buffer to store pointers to each argument.
        // * `argv_data`: the buffer to store arguments data.
        //
        // # Return
        // A WASI errno.
        SystemCall::ArgsGet => {
            let argv_pointers: u32 = wasmi_args.nth_checked(0).unwrap();
            let argv_data: u32 = wasmi_args.nth_checked(1).unwrap();
            args_or_env_get(theseus_args, argv_pointers, argv_data, memory)
        }

        // Return time value of a clock. This is similar to clock_gettime in POSIX.
        //
        // # Arguments
        // * `clock_id`: The clock for which to return the time.
        // * `precision`: The maximum lag (exclusive) that the returned time value may have, compared to its
        // actual value.
        // * `ret_ptr`: The buffer in which to store the time.
        //
        // # Return
        // A WASI errno.
        SystemCall::ClockTimeGet => {
            let clock_id: wasi::Clockid = wasmi_args.nth_checked(0).unwrap();
            let _precision: wasi::Timestamp = wasmi_args.nth_checked(1).unwrap();

            // TODO: use rtc value converted to unix timestamp.
            let unix_timestamp = 0;

            // fetch time.
            let timestamp: wasi::Timestamp = match clock_id {
                wasi::CLOCKID_MONOTONIC => unimplemented!(),
                wasi::CLOCKID_PROCESS_CPUTIME_ID => unimplemented!(),
                wasi::CLOCKID_REALTIME => unix_timestamp,
                wasi::CLOCKID_THREAD_CPUTIME_ID => unimplemented!(),
                _ => {
                    return Ok(Some(RuntimeValue::I32(From::from(wasi::ERRNO_NOTSUP))));
                }
            };

            // write time to return pointer.
            let ret_ptr: u32 = wasmi_args.nth_checked(2).unwrap();
            memory.set(ret_ptr, &timestamp.to_le_bytes()).unwrap();
            Ok(Some(RuntimeValue::I32(From::from(wasi::ERRNO_SUCCESS))))
        }
    }
}
