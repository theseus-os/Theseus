#![no_std]

#[macro_use]
mod wasi_definitions;
mod posix_file_system;
mod wasi_syscalls;
mod wasmi_state_machine;

#[macro_use]
extern crate alloc;
#[macro_use]
extern crate app_io;
extern crate root;
extern crate task;
extern crate wasmi;

use alloc::string::String;
use alloc::vec::Vec;
use posix_file_system::FileDescriptorTable;
use wasi_definitions::SystemCall;
use wasmi::{
    Error, Externals, FuncInstance, FuncRef, MemoryRef, Module, ModuleImportResolver, RuntimeArgs,
    RuntimeValue, Signature, Trap, ValueType,
};

pub struct HostExternals {
    memory: Option<MemoryRef>,
    exit_code: wasi::Exitcode,
    fd_table: FileDescriptorTable,
    theseus_env_vars: Vec<String>,
    theseus_args: Vec<String>,
}

impl Externals for HostExternals {
    fn invoke_index(
        &mut self,
        index: usize,
        wasmi_args: RuntimeArgs,
    ) -> Result<Option<RuntimeValue>, Trap> {
        wasi_syscalls::execute_system_call(SystemCall::from_usize(index).unwrap(), self, wasmi_args)
    }
}

impl ModuleImportResolver for HostExternals {
    fn resolve_func(&self, field_name: &str, signature: &Signature) -> Result<FuncRef, Error> {
        let system_call: SystemCall = match SystemCall::from_fn_name(field_name) {
            Ok(v) => v,
            Err(_) => {
                return Err(Error::Instantiation(format!(
                    "Export {} not found",
                    field_name
                )))
            }
        };

        if !signature.eq(&system_call.signature()) {
            return Err(Error::Instantiation(format!(
                "Export {} has a bad signature",
                field_name
            )));
        }

        Ok(FuncInstance::alloc_host(
            sig!((I32)),
            system_call.to_usize(),
        ))
    }
}

pub fn execute_binary(wasm_binary: Vec<u8>, args: Vec<String>) -> isize {
    // Load wasm binary and prepare it for instantiation.
    let module = Module::from_buffer(&wasm_binary).unwrap();

    let state_machine = wasmi_state_machine::ProcessStateMachine::new(
        &module,
        |wasm_interface: &str, fn_name: &str, fn_signature: &Signature| {
            if wasm_interface.eq("wasi_snapshot_preview1") {
                let system_call: SystemCall = match SystemCall::from_fn_name(fn_name) {
                    Ok(v) => v,
                    Err(_) => {
                        return Err(());
                    }
                };
                if fn_signature.eq(&system_call.signature()) {
                    return Ok(system_call.to_usize());
                }
            }
            Err(())
        },
    )
    .unwrap();

    let pwd: String = task::get_my_current_task()
        .unwrap()
        .get_env()
        .lock()
        .get_wd_path();
    let task_name: String = task::get_my_current_task().unwrap().name.clone();

    // Populate environment variables
    let mut theseus_env_vars: Vec<String> = Vec::new();
    theseus_env_vars.push(format!("PWD={}", pwd));

    // Populate args (POSIX-style)
    let mut theseus_args: Vec<String> = Vec::new();
    theseus_args.push(task_name);
    theseus_args.append(&mut args.clone());

    let mut ext: HostExternals = HostExternals {
        memory: state_machine.memory,
        exit_code: 0,
        fd_table: FileDescriptorTable::new(),
        theseus_env_vars: theseus_env_vars,
        theseus_args: theseus_args,
    };

    // TODO: Possibly need to open a FD with name of '.' in order to give access to PWD?
    let root_fd: wasi::Fd = ext
        .fd_table
        .open_path(
            root::ROOT_DIRECTORY_NAME,
            root::get_root().clone(),
            wasi::LOOKUPFLAGS_SYMLINK_FOLLOW,
            wasi::OFLAGS_DIRECTORY,
            wasi_definitions::FULL_DIR_RIGHTS,
            wasi_definitions::FULL_FILE_RIGHTS | wasi_definitions::FULL_DIR_RIGHTS,
            0,
        )
        .unwrap();

    match state_machine.module.invoke_export("_start", &[], &mut ext) {
        Ok(_) => {}
        Err(_) => {}
    };

    ext.fd_table.close_fd(root_fd).unwrap();

    ext.exit_code as isize
}
