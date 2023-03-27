//! A simple frontend to test `wasmtime` functionality on Theseus.
//! 
//! Currently, most of the tests are in the [wasmtime_runner] crate,
//! which allows the `wasmtime` crates to be a dependency of the Theseus kernel.

#![no_std]

#[macro_use] extern crate alloc;
#[macro_use] extern crate app_io;

use alloc::{
    string::{String, ToString},
    vec::Vec,
};
use path::Path;
use anyhow::Result;
use wasmtime::*;


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
    let path_to_hello_cwasm = Path::new(args.get(0).cloned().unwrap_or("/extra_files/wasm/hello.cwasm".to_string()));
    let Ok(curr_wd) = task::with_current_task(|t| t.get_env().lock().working_dir.clone()) else {
        return Err("failed to get current task".to_string());
    };

    let file = path_to_hello_cwasm.get_file(&curr_wd)
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

    run_hello_world(bytes.as_slice())
        .map_err(|e| format!("{}", e))?;

    Ok(())
}



/// Taken from `wasmtime/crates/wasmtime/src/lib.rs` docs example code.
pub fn run_hello_world(hello_world_cwasm_contents: &[u8]) -> Result<()> {
    // Modules can be compiled through either the text or binary format
    let engine = Engine::default();
    // Theseus note: `Module::new()` requires `#[cfg(compiler)]` for wasmtime,
    // such that it can perform JIT compilation of the WASM binary. 
    // We currently don't support that, so we have to use `Module::deserialize()`.
    // Old code: 
    // ```
    // let wat = r#"
    //     (module
    //         (import "host" "hello" (func $host_hello (param i32)))
    //         (func (export "hello")
    //             i32.const 3
    //             call $host_hello)
    //     )
    // "#;
    // let module = Module::new(&engine, wat)?;
    // ```
    let module = unsafe {
        Module::deserialize(&engine, hello_world_cwasm_contents)? 
    };
    // All wasm objects operate within the context of a "store". Each
    // `Store` has a type parameter to store host-specific data, which in
    // this case we're using `4` for.
    let mut store = Store::new(&engine, 4);
    let host_hello = Func::wrap(&mut store, |caller: Caller<'_, u32>, param: i32| {
        println!("Got {} from WebAssembly", param);
        println!("my host state is: {}", caller.data());
    });
    // Instantiation of a module requires specifying its imports and then
    // afterwards we can fetch exports by name, as well as asserting the
    // type signature of the function with `get_typed_func`.
    let instance = Instance::new(&mut store, &module, &[host_hello.into()])?;
    let hello = instance.get_typed_func::<(), (), _>(&mut store, "hello")?;
    // And finally we can call the wasm!
    hello.call(&mut store, ()).map_err(anyhow::Error::msg)?;
    Ok(())
}
