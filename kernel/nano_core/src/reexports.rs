//! This module is a shitty hack to re-export symbols from the compiler_builtins crate
//! as symbols with GLOBAL binding and DEFAULT visibility.
//! Without this, I tried to use `pub use::XXX` where XXX is one of the below function symbols,
//! but I could never figure out a proper linker command to force them to be linked with
//! DEFAULT visibility instead of HIDDEN visibility.
//! This is basically only required for loading libcore, which has a dependency on these functions.

use compiler_builtins;


// static REF_FLOATUNDIDF: extern "C" fn(u64) -> f64 = compiler_builtins::float::conv::__floatundidf;
// static REF_MULOTI4: extern "C" fn(i128, i128, &mut i32) -> i128 = compiler_builtins::int::mul::__muloti4;
// static REF_UDIVTI3: extern "C" fn(u128, u128) -> u128 = compiler_builtins::int::udiv::__udivti3;
// static REF_UMODTI3: extern "C" fn(u128, u128) -> u128 = compiler_builtins::int::udiv::__umodti3;


/// currently the compiler_builtins crate hasn't yet implemented this,
/// see: https://github.com/rust-lang-nursery/compiler-builtins/issues/216 
#[no_mangle]
pub fn __floatundisf(_a: i64) -> f32 {
    unimplemented!();
}
