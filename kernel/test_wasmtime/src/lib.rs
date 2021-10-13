//! A simple test crate for trying to build wasmtime
//! in a no_std environment, ported to Theseus.

#![no_std]

extern crate alloc;
#[macro_use] extern crate log;
extern crate wasmtime;
