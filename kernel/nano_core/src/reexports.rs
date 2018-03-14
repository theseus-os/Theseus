//! This module is a shitty hack to re-export symbols from the compiler_builtins crate
//! as symbols with GLOBAL binding and DEFAULT visibility.
//! Without this, I tried to use `pub use::XXX` where XXX is one of the below function symbols,
//! but I could never figure out a proper linker command to force them to be linked with
//! DEFAULT visibility instead of HIDDEN visibility.
//! This is basically only required for loading libcore, which has a dependency on these functions.

use compiler_builtins;


#[no_mangle]
pub fn __floatundisf(_a: i64) -> f32 {
    // currently the compiler_builtins crate hasn't yet implemented this,
    // see: https://github.com/rust-lang-nursery/compiler-builtins/issues/216 
    unimplemented!();
}

#[no_mangle]
pub fn __floatundidf(a: u64) -> f64 {
    compiler_builtins::float::conv::__floatundidf(a)
}

#[no_mangle]
pub fn __muloti4(a: i128, b: i128, oflow: &mut i32) -> i128 {
    compiler_builtins::int::mul::__muloti4(a, b, oflow)
}

#[no_mangle]
pub fn __udivti3(n: u128, d: u128) -> u128 {
    compiler_builtins::int::udiv::__udivti3(n, d)
}

#[no_mangle]
pub fn __umodti3(n: u128, d: u128) -> u128 {
    compiler_builtins::int::udiv::__umodti3(n, d)
}