#![no_std]

extern crate alloc;
extern crate wasmi;

#[macro_use]
extern crate terminal_print;

use alloc::string::String;
use alloc::vec::Vec;
use wasmi::{ImportsBuilder, ModuleInstance, NopExternals, RuntimeValue};

pub fn main(_args: Vec<String>) -> isize {
    // Load wasm binary as byte vector
    let wasm_binary: Vec<u8> = include_bytes!("test.wasm").to_vec();

    // Load wasm binary and prepare it for instantiation.
    let module = wasmi::Module::from_buffer(&wasm_binary).expect("failed to load wasm");

    // Instantiate a module with empty imports and
    // assert that there is no `start` function.
    let instance = ModuleInstance::new(&module, &ImportsBuilder::default())
        .expect("failed to instantiate wasm module")
        .assert_no_start();

    assert_eq!(
        instance
            .invoke_export("test", &[], &mut NopExternals,)
            .expect("failed to execute export"),
        Some(RuntimeValue::I32(1337)),
    );

    println!("wasmi test successfully executed.");

    0
}
