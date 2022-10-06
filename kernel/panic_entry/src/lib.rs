//! Provides the default entry points and lang items for panics and unwinding. 
//! 
//! These lang items are required by the Rust compiler. 
//! They should never be directly invoked by developers, only by the compiler. 
//! 

#![no_std]
#![feature(alloc_error_handler)]
#![feature(lang_items)]
#![feature(panic_info_message)]

extern crate alloc;
#[macro_use] extern crate log;
#[macro_use] extern crate vga_buffer;
extern crate memory;
extern crate mod_mgmt;
#[cfg(not(loadable))] extern crate panic_wrapper;
#[cfg(not(loadable))] extern crate unwind;

use core::panic::PanicInfo;

/// The singular entry point for a language-level panic.
/// 
/// The Rust compiler will rename this to "rust_begin_unwind".
#[cfg(not(test))]
#[panic_handler] // same as:  #[lang = "panic_impl"]
#[doc(hidden)]
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
    loop { }
}



/// Typically this would be an entry point in the unwinding procedure, in which a stack frame is unwound. 
/// However, in Theseus we use our own unwinding flow which is simpler.
/// 
/// This function will always be renamed to "rust_eh_personality" no matter what function name we give it here.
#[cfg(not(test))]
#[lang = "eh_personality"]
#[no_mangle]
#[doc(hidden)]
extern "C" fn rust_eh_personality() -> ! {
    error!("BUG: Theseus does not use rust_eh_personality. Why has it been invoked?");
    loop { }
}

/// This function is automatically jumped to after each unwinding cleanup routine finishes executing,
/// so it's basically the return address of every cleanup routine.
///
/// Just like the panic_entry_point() above, this is effectively just an entry point
/// that invokes the real `unwind_resume()` function in the `unwind` crate, 
/// but does so dynamically in loadable mode.
#[no_mangle]
extern "C" fn _Unwind_Resume(arg: usize) -> ! {
    #[cfg(not(loadable))] {
        unwind::unwind_resume(arg)
    }
    #[cfg(loadable)] {
        // An internal function for calling the real unwind_resume function, but returning errors along the way.
        // We must make sure to not hold any locks when invoking the function.
        fn invoke_unwind_resume(arg: usize) -> Result<(), &'static str> {
            type UnwindResumeFunc = fn(usize) -> !;
            const UNWIND_RESUME_SYMBOL: &'static str = "unwind::unwind_resume::";
            let section = {
                mod_mgmt::get_initial_kernel_namespace()
                    .and_then(|namespace| namespace.get_symbol_starting_with(UNWIND_RESUME_SYMBOL).upgrade())
                    .ok_or("Couldn't get single symbol matching \"unwind::unwind_resume::\"")?
            };
            let func: &UnwindResumeFunc = unsafe { section.as_func() }?;
            func(arg)
        }
        match invoke_unwind_resume(arg) {
            Ok(()) => error!("BUG: _Unwind_Resume: unexpectedly returned Ok(()) from unwind::unwind_resume()"),
            Err(e) => error!("_Unwind_Resume: failed to dynamically invoke unwind::unwind_resume! Error: {}", e),
        }
        loop { }
    }
}

/// This is the callback entry point that gets invoked when the heap allocator runs out of memory.
#[alloc_error_handler]
#[cfg(not(test))]
fn oom(_layout: core::alloc::Layout) -> ! {
    error!("\n(oom) Out of Heap Memory! requested allocation: {:?}", _layout);
    panic!("\n(oom) Out of Heap Memory! requested allocation: {:?}", _layout);
}
