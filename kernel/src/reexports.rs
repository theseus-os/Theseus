//! This module is a shitty hack to allow loading of crates that depend upon certain compiler_builtins. 
//! This is basically only required for loading libcore, which has a dependency on these functions.


/// currently the compiler_builtins crate hasn't yet implemented this,
/// see: https://github.com/rust-lang-nursery/compiler-builtins/issues/216 
#[no_mangle]
pub fn __floatundisf(_a: i64) -> f32 {
    unimplemented!();
}
