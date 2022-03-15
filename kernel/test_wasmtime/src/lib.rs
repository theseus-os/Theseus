//! A simple test crate for trying to build wasmtime
//! in a no_std environment, ported to Theseus.

#![no_std]

// extern crate alloc;
// #[macro_use] extern crate log;

// extern crate wasmparser; // wasmparser is working on no_std
// extern crate cranelift_entity; // cranelift-entity (with the "enable-serde" feature) is working on no_std
// extern crate wasmtime_types; // wasmtime-types is working on no_std
// extern crate wasmtime_environ;  // wasmtime-environ is working on no_std
// extern crate region;  // region is working on no_std

extern crate wasmtime_runtime;  // wasmtime-runtime is a WIP on no_std

// extern crate wasmtime;

