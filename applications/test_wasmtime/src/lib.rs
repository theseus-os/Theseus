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
use path::Path;


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
    let path_to_hello_cwasm = Path::new(args[0].clone());
    let curr_dir = task::get_my_current_task()
        .map(|t| t.get_env().lock().working_dir.clone())
        .ok_or_else(|| format!("Failed to get task's current working dir"))?;

    let file = path_to_hello_cwasm.get_file(&curr_dir)
        .ok_or_else(|| format!("Failed to get file at {:?}", path_to_hello_cwasm))?;

    let file_len = file.lock().len();
    let mut bytes = vec![0u8; file_len];
    let _bytes_read = file.lock().read_at(&mut bytes[..], 0)
        .map_err(|e| format!("{:?}", e))?;

    if _bytes_read != file_len {
        return Err(format!(
            "Short read: only read {} of {} bytes from file {}", 
            _bytes_read, file_len, path_to_hello_cwasm
        ));
    }

    wasmtime_runner::run_hello_world(bytes.as_slice())
        .map_err(|e| format!("{}", e))?;

    Ok(())
}
