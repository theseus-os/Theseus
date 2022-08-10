//! Provides the default entry point for unwinding.
//!
//! This crate defines symbols not present in `std` and can thus can be a dependency of `std`.
//! `panic_entry` defines symbols present in `std` and if it's included as a dependency, it results
//! in "duplicate lang_item" errors.

#![feature(lang_items)]
#![no_std]

use log::error;

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
    loop {}
}

/// This function is automatically jumped to after each unwinding cleanup routine finishes executing,
/// so it's basically the return address of every cleanup routine.
///
/// Just like the panic_entry_point() above, this is effectively just an entry point
/// that invokes the real `unwind_resume()` function in the `unwind` crate, 
/// but does so dynamically in loadable mode.
#[no_mangle]
pub extern "C" fn _Unwind_Resume(arg: usize) -> ! {
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
