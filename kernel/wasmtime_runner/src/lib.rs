//! A simple test crate for trying to build wasmtime
//! in a no_std environment, ported to Theseus.

#![no_std]

extern crate alloc;
#[macro_use] extern crate log;
#[macro_use] extern crate terminal_print;

// extern crate wasmparser; // wasmparser is working on no_std
// extern crate cranelift_entity; // cranelift-entity (with the "enable-serde" feature) is working on no_std
// extern crate wasmtime_types; // wasmtime-types is working on no_std
// extern crate wasmtime_environ;  // wasmtime-environ is working on no_std
// extern crate region;  // region is working on Theseus

// extern crate wasmtime_runtime;  // wasmtime-runtime builds on Theseus

// extern crate jit; // WIP on Theseus
// extern crate wasmtime; // WIP on Theseus



use anyhow::Result;
use wasmtime::*;


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
