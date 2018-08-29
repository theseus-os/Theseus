//! Provides the default entry points and lang items for panics and unwinding. 
//! 
//! These lang items are required by the Rust compiler. 
//! They should never be directly invoked by developers, only by the compiler. 
//! 

#![no_std]
#![feature(lang_items)]

#[macro_use] extern crate log;




#[cfg(not(test))]
#[lang = "eh_personality"]
#[no_mangle]
#[doc(hidden)]
pub extern "C" fn eh_personality() {}


#[cfg(not(test))]
#[lang = "panic_fmt"]
#[no_mangle]
#[doc(hidden)]
pub extern "C" fn panic_fmt(fmt_args: core::fmt::Arguments, file: &'static str, line: u32, col: u32) -> ! {
    error!("SIMPLE PANIC in {}:{}:{} -- {}", file, line, col, fmt_args);
    loop {}
}



/// This function isn't used since our Theseus target.json file
/// chooses panic=abort (as does our build process), 
/// but building on Windows (for an IDE) with the pc-windows-gnu toolchain requires it.
#[allow(non_snake_case)]
#[lang = "eh_unwind_resume"]
#[no_mangle]
#[cfg(all(target_os = "windows", target_env = "gnu"))]
#[doc(hidden)]
pub extern "C" fn rust_eh_unwind_resume(_arg: *const i8) -> ! {
    error!("\n\nin rust_eh_unwind_resume, unimplemented!");
    loop {}
}


#[allow(non_snake_case)]
#[no_mangle]
#[cfg(not(target_os = "windows"))]
#[doc(hidden)]
pub extern "C" fn _Unwind_Resume() -> ! {
    error!("\n\nin _Unwind_Resume, unimplemented!");
    loop {}
}
