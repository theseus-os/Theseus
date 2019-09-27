//! Provides the default entry points and lang items for panics and unwinding. 
//! 
//! These lang items are required by the Rust compiler. 
//! They should never be directly invoked by developers, only by the compiler. 
//! 

#![no_std]
#![feature(alloc_error_handler)]
#![feature(lang_items)]
#![feature(panic_info_message)]
#![feature(asm, naked_functions)]
#![feature(unwind_attributes)]
#![feature(trait_alias)]

extern crate alloc;
#[macro_use] extern crate log;
#[macro_use] extern crate vga_buffer;
extern crate memory;
extern crate panic_wrapper;
extern crate mod_mgmt;
extern crate irq_safety;
extern crate task;
extern crate gimli;
extern crate fallible_iterator;
extern crate scheduler;
extern crate apic;
extern crate runqueue;

mod registers;
mod unwind;
mod lsda;

use core::panic::PanicInfo;
use memory::VirtualAddress;
use unwind::{NamespaceUnwinder, StackFrameIter};
use fallible_iterator::FallibleIterator;
use gimli::{BaseAddresses, NativeEndian};
use mod_mgmt::{
    CrateNamespace,
    metadata::{StrongCrateRef, StrongSectionRef, SectionType},
};
use alloc::boxed::Box;
use task::{PanicInfoOwned, KillReason};



/// The singular entry point for a language-level panic.
#[cfg(not(test))]
#[panic_handler] // same as:  #[lang = "panic_impl"]
#[doc(hidden)]
fn panic_unwind_test(info: &PanicInfo) -> ! {
    trace!("panic_unwind_test() [top]: {:?}", info);

    // Call this task's panic handler, if it has one.
    // Note that we must consume and drop the Task's panic handler BEFORE that Task can possibly be dropped.
    // This is because if the app sets a panic handler that is a closure/function in the text section of the app itself,
    // then after the app crate is released the panic handler will be dropped AFTER the app crate has been freed.
    // When it tries to drop the task's panic handler, causes a page fault because the text section of the app crate has been unmapped.
    {
        let panic_handler = task::get_my_current_task().and_then(|t| t.take_panic_handler());
        if let Some(ref ph_func) = panic_handler {
            debug!("Found panic handler callback to invoke in Task {:?}", task::get_my_current_task());
            ph_func(info);
        }
        else {
            debug!("No panic handler callback in Task {:?}", task::get_my_current_task());
        }
    }
    
    // We should ensure that the lock on the curr_task isn't held here,
    // in order to allow the panic handler and other functions below to acquire it. 


    // let mut rbp: usize;
    // let mut rsp: usize;
    // let mut rip: usize;
    // unsafe {
    //     // On x86 you cannot directly read the value of the instruction pointer (RIP),
    //     // so we use a trick that exploits RIP-relateive addressing to read the current value of RIP (also gets RBP and RSP)
    //     asm!("lea $0, [rip]" : "=r"(rip), "={rbp}"(rbp), "={rsp}"(rsp) : : "memory" : "intel", "volatile");
    // }
    // debug!("panic_unwind_test(): register values: RIP: {:#X}, RSP: {:#X}, RBP: {:#X}", rip, rsp, rbp);
    // let _curr_instruction_pointer = VirtualAddress::new_canonical(rip);

    // let starting_instruction_pointer = panic_wrapper::get_first_non_panic_instruction_pointer(
    //     &mmi_ref.lock().page_table,
    //     &|instruction_pointer: VirtualAddress| {
    //         namespace.get_section_containing_address(instruction_pointer, app_crate_ref.as_ref(), false)
    //             .map(|(sec_ref, offset)| (sec_ref.lock().name.clone(), offset))
    //     },
    // ).expect("couldn't determine which instruction pointer was the first non-panic-related one");
    // debug!("panic_unwind_test(): starting search at RIP: {:#X}", starting_instruction_pointer);
    // // search for unwind entry related to the current instruction pointer
    // // first, we need to get the containing crate for this IP
    // let my_crate = namespace.get_crate_containing_address(starting_instruction_pointer, app_crate_ref.as_ref(), false)
    //     .expect("panic_unwind_test(): couldn't get crate containing address");
    // debug!("panic_unwind_test(): looking at crate {:?}", my_crate);


    // // dump some info about the loaded app crate
    // if let Some(ref app_crate) = app_crate_ref {
    //     let krate = app_crate.lock_as_ref();
    //     trace!("============== Crate {} =================", krate.crate_name);
    //     for s in krate.sections.values() {
    //         trace!("   {:?}", &*s.lock());
    //     }
    // }
    let cause = KillReason::Panic(PanicInfoOwned::from(info));
    match unwind::start_unwinding(cause) {
        Ok(_) => {
            warn!("BUG: start_unwinding() returned an Ok() value, which is unexpected because it means no unwinding actually occurred. Task: {:?}.", task::get_my_current_task());
        }
        Err(e) => {
            error!("Task {:?} was unable to start unwinding procedure, error: {}.", task::get_my_current_task(), e);
        }
    }

    warn!("Looping at the end of panic_unwind_test()!");
    loop { }
}



/// The singular entry point for a language-level panic.
#[cfg(not(test))]
// #[panic_handler] // same as:  #[lang = "panic_impl"]
#[doc(hidden)]
fn panic_entry_point(info: &PanicInfo) -> ! {
    // Since a panic could occur before the memory subsystem is initialized,
    // we must check before using alloc types or other functions that depend on the memory system (the heap).
    // We can check that by seeing if the kernel mmi has been initialized.
    let kernel_mmi_ref = memory::get_kernel_mmi_ref();  
    let res = if kernel_mmi_ref.is_some() {
        // proceed with calling the panic_wrapper, but don't shutdown with try_exit() if errors occur here
        #[cfg(not(loadable))]
        {
            panic_wrapper::panic_wrapper(info)
        }
        #[cfg(loadable)]
        {
            // An internal function for calling the panic_wrapper, but returning errors along the way.
            // We must make sure to not hold any locks when invoking the panic_wrapper function.
            fn invoke_panic_wrapper(info: &PanicInfo) -> Result<(), &'static str> {
                type PanicWrapperFunc = fn(&PanicInfo) -> Result<(), &'static str>;
                let section_ref = mod_mgmt::get_default_namespace()
                    .and_then(|namespace| namespace.get_symbol_starting_with("panic_wrapper::panic_wrapper::").upgrade())
                    .ok_or("Couldn't get single symbol matching \"panic_wrapper::panic_wrapper\"")?;
                let (mapped_pages, mapped_pages_offset) = { 
                    let section = section_ref.lock();
                    (section.mapped_pages.clone(), section.mapped_pages_offset)
                };
                let mut space = 0;
                let func: &PanicWrapperFunc = {
                    mapped_pages.lock().as_func(mapped_pages_offset, &mut space)?
                };
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
    // But in general, the thread should be killed by the default panic handler, so it shouldn't reach here.
    // Only panics early on in the initialization process will get here, meaning that the OS will basically stop.
    
    loop {}
}



/// Typically this would be an entry point in the unwinding procedure, in which a stack frame is unwound. 
/// However, in Theseus we use our own unwinding flow which is simpler.
/// 
/// This function will always be renamed to "rust_eh_personality" no matter what function name we give it here.
#[cfg(not(test))]
#[lang = "eh_personality"]
#[no_mangle]
#[doc(hidden)]
extern "C" fn rust_eh_personality() {
    error!("BUG: Theseus does not use rust_eh_personality. Why has it been invoked?");
}


/// This is the callback entry point that gets invoked when the heap allocator runs out of memory.
#[alloc_error_handler]
#[cfg(not(test))]
fn oom(_layout: core::alloc::Layout) -> ! {
    println_raw!("\nOut of Heap Memory! requested allocation: {:?}", _layout);
    error!("Out of Heap Memory! requested allocation: {:?}", _layout);
    loop {}
}
