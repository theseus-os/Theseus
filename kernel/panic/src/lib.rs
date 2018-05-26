//! Provides types and simple routines for handling panics.
//! This is similar to what's found in Rust's `core::panic` crate, 
//! but is much simpler and doesn't require lifetimes 
//! (although it does require alloc types like String).
//! 
#![no_std]
#![feature(alloc)]

#[macro_use] extern crate alloc;
// #[macro_use] extern crate log;


use core::fmt;
use alloc::String;
use alloc::boxed::Box;


/// Contains details about where a panic occurred.
#[derive(Debug)]
pub struct PanicLocation {
    file: String,
    line: u32, 
    col: u32,
}
impl PanicLocation {
    /// Create a new PanicLocation
    pub fn new<S: Into<String>>(file: S, line: u32, col: u32) -> PanicLocation {
        PanicLocation {
            file: file.into(),
            line: line,
            col:  col,
        }
    }
}


/// Similar to the core library's PanicInfo, but simpler. 
#[derive(Debug)]
pub struct PanicInfo {
    location: PanicLocation,
    msg: String,
}
impl PanicInfo {
    /// Create a new PanicInfo struct with the given message `String`.
    pub fn new<S: Into<String>>(file: S, line: u32, col: u32, msg: S) -> PanicInfo {
        Self::with_location(PanicLocation::new(file, line, col), msg)
    }

    /// Create a new PanicInfo struct with the given message `String`.
    pub fn with_location<S: Into<String>>(location: PanicLocation, msg: S) -> PanicInfo {
        PanicInfo {
            location: location,
            msg: msg.into(), 
        }
    }
    
    /// Create a new PanicInfo struct from `core::fmt::Arguments` message.
    pub fn with_fmt_args(location: PanicLocation, msg: fmt::Arguments) -> PanicInfo {
        Self::with_location(location, format!("{}", msg), )
    }
}


/// The signature of the callback function that can hook into receiving a panic. 
/// TODO change this to a closure, i.e., whatever type panic::set_hook() accepts in the core library.
// pub type PanicHandler = fn(&PanicInfo);
pub type PanicHandler = Box<Fn(&PanicInfo)>;