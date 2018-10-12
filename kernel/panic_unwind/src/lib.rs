//! Provides the default entry points and lang items for panics and unwinding. 
//! 
//! These lang items are required by the Rust compiler. 
//! They should never be directly invoked by developers, only by the compiler. 
//! 

#![no_std]
#![feature(lang_items)]
#![feature(alloc)]

extern crate alloc;
#[macro_use] extern crate log;
#[macro_use] extern crate vga_buffer;
extern crate memory;
extern crate panic_wrapper;
extern crate mod_mgmt;


use core::fmt;


#[cfg(not(test))]
#[lang = "eh_personality"]
#[no_mangle]
#[doc(hidden)]
pub extern "C" fn eh_personality() {}


#[cfg(not(test))]
#[lang = "panic_fmt"]
#[no_mangle]
#[doc(hidden)]
pub extern "C" fn panic_fmt(fmt_args: fmt::Arguments, file: &'static str, line: u32, col: u32) -> ! {
    
    // Since a panic could occur before the memory subsystem is initialized,
    // we must check before using alloc types or other functions that depend on the memory system (the heap).
    // We can check that by seeing if the kernel mmi has been initialized.
    let kernel_mmi_ref = memory::get_kernel_mmi_ref();  
    let res = if kernel_mmi_ref.is_some() {
        // proceed with calling the panic_wrapper, but don't shutdown with try_exit() if errors occur here
        #[cfg(loadable)]
        {
            use core::ops::DerefMut;
            use mod_mgmt::metadata::CrateType;
            
            type PanicWrapperFunc = fn(fmt_args: fmt::Arguments, file: &'static str, line: u32, col: u32) -> Result<(), &'static str>;
            let section_ref = kernel_mmi_ref.and_then(|kernel_mmi| {
                mod_mgmt::get_default_namespace().get_symbol_or_load("panic_wrapper::panic_wrapper", CrateType::Kernel.prefix(), None, kernel_mmi.lock().deref_mut(), false).upgrade()
            }).ok_or("Couldn't get symbol: \"panic_wrapper::panic_wrapper\"");

            // call the panic_wrapper function, otherwise return an Err into "res"
            let mut space = 0;
            section_ref.and_then(|section_ref| {
                let (mapped_pages, mapped_pages_offset) = { 
                    let section = section_ref.lock();
                    (section.mapped_pages.clone(), section.mapped_pages_offset)
                };
                let mapped_pages_locked = mapped_pages.lock();
                mapped_pages_locked.as_func::<PanicWrapperFunc>(mapped_pages_offset, &mut space)
                    .and_then(|func| func(fmt_args, file, line, col)) // actually call the function
            })
        }
        #[cfg(not(loadable))]
        {
            panic_wrapper::panic_wrapper(fmt_args, file, line, col)
        }
    }
    else {
        Err("memory subsystem not yet initialized, cannot call panic_wrapper because it requires alloc types")
    };

    if let Err(_e) = res {
        // basic early panic printing with no dependencies
        println_raw!("\nPANIC in {}:{}:{} -- {}", file, line, col, fmt_args);
        error!("PANIC in {}:{}:{} -- {}", file, line, col, fmt_args);
    }

    // if we failed to handle the panic, there's not really much we can do about it
    // other than just let the thread spin endlessly (which doesn't hurt correctness but is inefficient). 
    // But in general, the thread should be killed by the default panic handler, so it shouldn't reach here.
    // Only panics early on in the initialization process will get here, meaning that the OS will basically stop.
    
    loop {}
}



// /// This function isn't used since our Theseus target.json file
// /// chooses panic=abort (as does our build process), 
// /// but building on Windows (for an IDE) with the pc-windows-gnu toolchain requires it.
// #[allow(non_snake_case)]
// #[lang = "eh_unwind_resume"]
// #[no_mangle]
// #[cfg(all(target_os = "windows", target_env = "gnu"))]
// #[doc(hidden)]
// pub extern "C" fn rust_eh_unwind_resume(_arg: *const i8) -> ! {
//     error!("\n\nin rust_eh_unwind_resume, unimplemented!");
//     loop {}
// }


#[allow(non_snake_case)]
#[no_mangle]
#[cfg(not(target_os = "windows"))]
#[doc(hidden)]
pub extern "C" fn _Unwind_Resume() -> ! {
    error!("\n\nin _Unwind_Resume, unimplemented!");
    loop {}
}
