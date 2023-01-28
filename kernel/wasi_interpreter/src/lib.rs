//! Interpreter for executing WASI-compliant WASM binaries.
//!
//! `wasi_interpreter` provides an interface between the `wasmi` crate (used to interpret
//! WebAssembly) and Theseus under the assumption of a WASI interface.
//!
//! This library exposes a public method called `execute_binary` to allow for the execution of a
//! WebAssembly binary given directory permissions (in accordance to the WASI capabilities model)
//! and arguments.
//!
//! This library depends on the following modules:
//! * wasi_definitions
//! * wasi_syscalls
//! * posix_file_system
//! * wasmi_state_machine
//!

#![no_std]

#[macro_use]
mod wasi_definitions;
mod posix_file_system;
mod wasi_syscalls;
mod wasmi_state_machine;

#[macro_use]
extern crate alloc;
extern crate app_io;
extern crate core2;
extern crate fs_node;
extern crate root;
extern crate task;
extern crate wasmi;

use alloc::string::String;
use alloc::vec::Vec;
use core::convert::TryFrom;
use core::str::FromStr;
use posix_file_system::FileDescriptorTable;
use wasi_definitions::SystemCall;
use wasmi::{Externals, MemoryRef, Module, RuntimeArgs, RuntimeValue, Signature, Trap, ValueType};

/// Theseus and wasmi I/O required to execute WASI system calls.
pub struct HostExternals {
    /// WebAssembly memory buffer provided by wasmi.
    memory: Option<MemoryRef>,
    /// Exit code returned WebAssembly binary.
    exit_code: wasi::Exitcode,
    /// POSIX-style file descriptor table abstraction for interacting with Theseus I/O.
    fd_table: FileDescriptorTable,
    /// POSIX-formatted environment variables provided by Theseus.
    /// (i.e. KEY=VALUE)
    theseus_env_vars: Vec<String>,
    /// POSIX-formatted arguments.
    /// (i.e. PROGRAM_NAME arg1 arg2 ...)
    theseus_args: Vec<String>,
}

impl Externals for HostExternals {
    /// Function used by wasmi to invoke a system call given a specified system call number and
    /// wasm arguments.
    fn invoke_index(
        &mut self,
        index: usize,
        wasmi_args: RuntimeArgs,
    ) -> Result<Option<RuntimeValue>, Trap> {
        wasi_syscalls::execute_system_call(SystemCall::try_from(index).unwrap(), self, wasmi_args)
    }
}

/// Executes a WASI-compliant WebAssembly binary.
///
/// This function constructs a wasmi state machine from a WebAssembly binary, constructs a
/// HostExternals object consisting of any necessary Theseus or wasmi I/O, opens file descriptors
/// for accessible directories, and executes.
///
/// # Arguments
/// * `wasm_binary`: a WASI-compliant WebAssembly binary as a byte vector
/// * `args`: a POSIX-formatted string vector of arguments to WebAssembly binary
/// * `preopen_dirs`: a string vector of directory paths to grant WASI access to
///
pub fn execute_binary(wasm_binary: Vec<u8>, args: Vec<String>, preopen_dirs: Vec<String>) -> isize {
    // Load wasm binary and prepare it for instantiation.
    let module = Module::from_buffer(&wasm_binary).unwrap();

    // Construct wasmi WebAssembly state machine.
    let state_machine = wasmi_state_machine::ProcessStateMachine::new(
        &module,
        |wasm_interface: &str, fn_name: &str, fn_signature: &Signature| -> Result<usize, ()> {
            // Match WebAssembly function import to corresponding system call number.
            // Currently supports `wasi_snapshot_preview1`.
            if wasm_interface.eq("wasi_snapshot_preview1") {
                let system_call = SystemCall::from_str(fn_name)
                    .unwrap_or_else(|_| panic!("Unknown WASI function {}", fn_name));
                // Verify that signature of system call matches expected signature.
                if fn_signature.eq(&system_call.into()) {
                    return Ok(system_call.into());
                }
            }
            Err(())
        },
    )
    .unwrap();

    // Populate environment variables.
    let pwd: String = task::with_current_task(|t|
        t.get_env().lock().cwd()
    ).expect("couldn't get current task");

    let mut theseus_env_vars: Vec<String> = Vec::new();
    theseus_env_vars.push(format!("PWD={pwd}"));

    // Construct initial host externals.
    let mut ext: HostExternals = HostExternals {
        memory: state_machine.memory,
        exit_code: 0,
        fd_table: FileDescriptorTable::new(),
        theseus_env_vars,
        theseus_args: args,
    };

    // Open permitted directories in file descriptor table prior to execution.
    // NOTE: WASI relies on an assumption that all preopened directories occupy the lowest possible
    // file descriptors (3, 4, ...). The `open_path` function below conforms to this standard.
    for preopen_dir in preopen_dirs.iter() {
        let _curr_fd: wasi::Fd = ext
            .fd_table
            .open_path(
                preopen_dir,
                task::with_current_task(|t|
                    t.get_env().lock().working_dir.clone()
                ).expect("couldn't get current task"),
                wasi::LOOKUPFLAGS_SYMLINK_FOLLOW,
                wasi::OFLAGS_DIRECTORY,
                wasi_definitions::FULL_DIR_RIGHTS,
                wasi_definitions::FULL_FILE_RIGHTS | wasi_definitions::FULL_DIR_RIGHTS,
                0,
            )
            .unwrap();
    }

    // Execute WebAssembly binary.
    state_machine
        .module
        .invoke_export("_start", &[], &mut ext)
        .ok();

    // Return resulting WebAssembly exit code.
    isize::try_from(ext.exit_code).unwrap()
}
