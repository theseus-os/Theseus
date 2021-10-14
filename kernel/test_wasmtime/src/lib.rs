//! A simple test crate for trying to build wasmtime
//! in a no_std environment, ported to Theseus.

#![no_std]

extern crate alloc;
#[macro_use] extern crate log;

// extern crate wasmparser; // wasmparser is working on no_std
extern crate cranelift_entity; // cranelift-entity is working on no_std with the "enable-serde" feature.

// extern crate wasmtime_types; 

// extern crate wasmtime;
