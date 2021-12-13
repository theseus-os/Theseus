//! pee (https://wapm.io/package/liftm/pee)
//! compiled to WebAssembly with WASI support and run with wasi_interpreter

#![no_std]

extern crate alloc;
extern crate wasi_interpreter;

use alloc::string::String;
use alloc::vec::Vec;

pub fn main(args: Vec<String>) -> isize {
    // Parse WAT (WebAssembly Text format) into wasm bytecode.
    let wasm_binary: Vec<u8> = include_bytes!("pee.wasm").to_vec();

    wasi_interpreter::execute_binary(wasm_binary, args)
}
