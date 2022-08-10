//! Provides the default entry points and lang items for panics and oom handlers.
//! 
//! These lang items are required by the Rust compiler - they should never be explicitly invoked.

#![no_std]
#![feature(alloc_error_handler)]
#![feature(lang_items)]
#![feature(panic_info_message)]

pub use panic_entry_inner as _;

use core::panic::PanicInfo;
use log::error;
use vga_buffer::println_raw;

/// The singular entry point for a language-level panic.
/// 
/// The Rust compiler will rename this to "rust_begin_unwind".
#[panic_handler] // same as:  #[lang = "panic_impl"]
#[doc(hidden)]
#[cfg(not(test))]
fn panic_entry_point(info: &PanicInfo) -> ! {
    // Since a panic could occur before the memory subsystem is initialized,
    // we must check before using alloc types or other functions that depend on the memory system (the heap).
    // We can check that by seeing if the kernel mmi has been initialized.
    let kernel_mmi_ref = memory::get_kernel_mmi_ref();  
    let res = if kernel_mmi_ref.is_some() {
        // proceed with calling the panic_wrapper, but don't shutdown with try_exit() if errors occur here
        #[cfg(not(loadable))] {
            panic_wrapper::panic_wrapper(info)
        }
        #[cfg(loadable)] {
            // An internal function for calling the panic_wrapper, but returning errors along the way.
            // We must make sure to not hold any locks when invoking the panic_wrapper function.
            fn invoke_panic_wrapper(info: &PanicInfo) -> Result<(), &'static str> {
                type PanicWrapperFunc = fn(&PanicInfo) -> Result<(), &'static str>;
                const PANIC_WRAPPER_SYMBOL: &'static str = "panic_wrapper::panic_wrapper::";
                let section = {
                    mod_mgmt::get_initial_kernel_namespace()
                        .and_then(|namespace| namespace.get_symbol_starting_with(PANIC_WRAPPER_SYMBOL).upgrade())
                        .ok_or("Couldn't get single symbol matching \"panic_wrapper::panic_wrapper::\"")?
                };
                let func: &PanicWrapperFunc = unsafe { section.as_func() }?;
                func(info)
            }

            // call the above internal function
            invoke_panic_wrapper(info)
        }
    }
    else {
        Err("memory subsystem not yet initialized, cannot call panic_wrapper because it requires alloc types")
    };

    if let Err(_e) = res {
        // basic early panic printing with no dependencies
        println_raw!("\nPANIC: {}", info);
        error!("PANIC: {}", info);
    }

    // If we failed to handle the panic, there's not really much we can do about it,
    // other than just let the thread spin endlessly (which doesn't hurt correctness but is inefficient). 
    // But in general, this task should be killed by the panic_wrapper, so it shouldn't reach this point.
    // Only panics early on in the initialization process will get here, meaning that the OS will basically stop.
    loop {}
}

/// This is the callback entry point that gets invoked when the heap allocator runs out of memory.
#[alloc_error_handler]
#[cfg(not(test))]
fn oom(_layout: core::alloc::Layout) -> ! {
    error!("\n(oom) Out of Heap Memory! requested allocation: {:?}", _layout);
    panic!("\n(oom) Out of Heap Memory! requested allocation: {:?}", _layout);
}
