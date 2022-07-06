
//! Simple C-compatible function that panics after a stack smash has occurred. 
//! 
//! This is used by C compiler instrumentation for C code that was compiled with 
//! support for stack smashing protection. 
//! Basically, when a stack smash is detected, the `__stack_chk_fail()` function
//! is invoked as a callback, which may take a variety of actions to report 
//! the stack smashing error.
//! 
//! OSdev wiki page: <https://wiki.osdev.org/Stack_Smashing_Protector>

/// See module docs. This shouldn't be invoked directly, but it's safe to do so
/// because all it does is panic.
#[no_mangle]
pub extern "C" fn __stack_chk_fail() -> ! {
	panic!("__stack_chk_fail: Stack smashing detected!");
}
