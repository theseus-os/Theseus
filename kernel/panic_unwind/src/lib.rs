#![no_std]
#![feature(lang_items)]

// #[macro_use] extern crate vga_buffer;


#[cfg(not(test))]
#[lang = "eh_personality"]
extern "C" fn eh_personality() {}


#[cfg(not(test))]
#[lang = "panic_fmt"]
#[no_mangle]
pub extern "C" fn panic_fmt(fmt: core::fmt::Arguments, file: &'static str, line: u32) -> ! {
    // println_unsafe!("\n\nPANIC in {} at line {}:", file, line);
    // println_unsafe!("    {}", fmt);

    // TODO: check out Redox's unwind implementation: https://github.com/redox-os/kernel/blob/b364d052f20f1aa8bf4c756a0a1ea9caa6a8f381/src/arch/x86_64/interrupt/trace.rs#L9

    loop {}
}


#[allow(non_snake_case)]
#[no_mangle]
pub extern "C" fn _Unwind_Resume() -> ! {
    // println_unsafe!("\n\nin _Unwind_Resume, unimplemented!");
    loop {}
}