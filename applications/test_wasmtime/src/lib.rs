//! A simple frontend to test `wasmtime` functionality on Theseus.
//! 
//! Currently, most of the tests are in the [wasmtime_runner] crate,
//! which allows the `wasmtime` crates to be a dependency of the Theseus kernel.

#![no_std]

#[macro_use] extern crate alloc;
#[macro_use] extern crate terminal_print;

use alloc::{
    string::String,
    vec::Vec,
};


pub fn main(args: Vec<String>) -> isize {
    match rmain(args) {
        Ok(_) => 0,
        Err(e) => {
            println!("Error: {}", e);
            -1
        }
    }
}


fn rmain(args: Vec<String>) -> Result<(), String> {
    wasmtime_runner::hello_world()
        .map_err(|e| format!("{}", e))?;

    Ok(())
}
