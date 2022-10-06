//! A Theseus-specific port of parts of the Rust standard library `std`.
//! 
//! Current ported modules include:
//! * `fs`: basic filesystem access.
//! * `os_str`: platform-native string types.
//!    * In Theseus, `OsString` = `String`, and `OsStr` = `str`.
//! * `path`: basic path representations: `PathBuf` and `Path`.
//! 

#![no_std]
#![feature(extend_one)]
#![feature(trait_alias)]

extern crate alloc;

mod env;
pub mod fs;
mod fs_imp;
pub mod os_str;
mod os_str_imp;
pub mod path;
mod sys_common;


// Taken from: <https://github.com/rust-lang/rust/blob/8834629b861cd182be6b914d4e6bc5958160debc/library/std/src/lib.rs#L625>
mod sealed {
    /// This trait being unreachable from outside the crate
    /// prevents outside implementations of our extension traits.
    /// This allows adding more trait methods in the future.
    pub trait Sealed {}
}
