#![no_std]

#[macro_use]
mod wasi_definitions;
mod wasi_syscalls;
mod wasmi_state_machine;

#[macro_use]
extern crate alloc;
#[macro_use]
extern crate terminal_print;
extern crate wasmi;

use alloc::string::String;
use alloc::vec::Vec;
use core::convert::TryFrom as _;
use wasi_definitions::SystemCall;
use wasmi::{
    Error, Externals, FuncInstance, FuncRef, MemoryRef, Module, ModuleImportResolver, RuntimeArgs,
    RuntimeValue, Signature, Trap, ValueType,
};

pub struct HostExternals {
    memory: Option<MemoryRef>,
}

impl Externals for HostExternals {
    fn invoke_index(
        &mut self,
        index: usize,
        args: RuntimeArgs,
    ) -> Result<Option<RuntimeValue>, Trap> {
        wasi_syscalls::execute_system_call(SystemCall::from_usize(index).unwrap(), self, args)
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

pub fn main() -> isize {
    // Parse WAT (WebAssembly Text format) into wasm bytecode.
    let wasm_binary: Vec<u8> = include_bytes!("hello.wasm").to_vec();

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

    let mut ext: HostExternals = HostExternals {
        memory: state_machine.memory,
    };

    state_machine
        .module
        .invoke_export("_start", &[], &mut ext)
        .expect("failed to invoke _start");
    0
}
