//! Provides the default entry points and lang items for panics and unwinding. 
//! 
//! These lang items are required by the Rust compiler. 
//! They should never be directly invoked by developers, only by the compiler. 
//! 

#![no_std]
#![feature(alloc_error_handler)]
#![feature(lang_items)]
#![feature(panic_info_message)]
#![feature(asm)]

extern crate alloc;
#[macro_use] extern crate log;
extern crate memory;
extern crate mod_mgmt;
extern crate task;
extern crate gimli;


use core::panic::PanicInfo;
use alloc::string::String;
use memory::{VirtualAddress, PageTable};


#[cfg(not(test))]
#[lang = "eh_personality"]
#[no_mangle]
#[doc(hidden)]
pub extern "C" fn eh_personality() {
    error!("EH_PERSONALITY IS UNHANDLED!");
}


#[cfg(not(test))]
#[panic_handler] // same as:  #[lang = "panic_impl"]
#[doc(hidden)]
fn panic_handler_simple(_info: &PanicInfo) -> ! {
    error!("IN PANIC_HANDLER_SIMPLE");
    
    let curr_task = task::get_my_current_task().expect("couldn't get my current task");
    let namespace = curr_task.get_namespace();
    let (mmi_ref, app_crate_ref) = { 
        let t = curr_task.lock();
        (t.mmi.clone(), t.app_crate.clone())
    };

    // print a stack trace
    stack_trace(
        &mmi_ref.lock().page_table,
        &|instruction_pointer: VirtualAddress| {
            namespace.get_containing_section(instruction_pointer, app_crate_ref.as_ref())
                .map(|(sec_ref, offset)| (sec_ref.lock().name.clone(), offset))
        },
    );
    
    let curr_task_locked = curr_task.lock();
    let my_crate = curr_task_locked.app_crate.as_ref().expect("unwind_test's app_crate was None");
    for sec_ref in my_crate.lock_as_ref().sections.values() {
        let sec = sec_ref.lock();
        trace!("    section {:?}, vaddr: {:#X}, size: {:#X}", sec.name, sec.virt_addr(), sec.size);
    }
    namespace.handle_eh_frame(&my_crate, true).expect("handle_eh_frame failed");


    error!("reached end of panic_handler_simple");
    loop { }
}



/// This function isn't used since our Theseus target.json file
/// chooses panic=abort (as does our build process), 
/// but building on Windows (for an IDE) with the pc-windows-gnu toolchain requires it.
#[allow(non_snake_case)]
#[lang = "eh_unwind_resume"]
#[no_mangle]
// #[cfg(all(target_os = "windows", target_env = "gnu"))]
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


#[alloc_error_handler]
#[cfg(not(test))]
fn oom(_layout: core::alloc::Layout) -> ! {
    error!("Out of Heap Memory! requested allocation: {:?}", _layout);
    loop {}
}




/// Get a stack trace, borrowed from Redox
/// TODO: Check for stack being mapped before dereferencing
#[inline(never)]
pub fn stack_trace(
    current_page_table: &PageTable,
    addr_to_symbol: &dyn Fn(VirtualAddress) -> Option<(String, usize)>) 
{

    // SAFE, just a stack trace for debugging purposes, and pointers are checked. 
    unsafe {
        
        // get the stack base pointer
        let mut rbp: usize;
        let mut rsp: usize;
        asm!("" : "={rbp}"(rbp), "={rsp}"(rsp) : : "memory" : "intel", "volatile");

        error!("STACK TRACE: RBP: {:>#018X}, RSP: {:>#018X}", rbp, rsp);
        if rbp == 0 {
            error!("Frame pointers have been omitted in this build. \
                Stack tracing/unwinding cannot be performed because we don't yet \
                support using DWARF .debug_* sections to backtrace the stack. \
                Make sure that the rustc option '-C force-frame-pointers=yes' is used."
            );
        }
        // set a recursion maximum of 64 stack frames
        for _frame in 0..64 {
            if let Some(rip_rbp) = rbp.checked_add(core::mem::size_of::<usize>()) {
                // TODO: is this the right condition?
                match (VirtualAddress::new(rbp), VirtualAddress::new(rip_rbp)) {
                    (Ok(rbp_vaddr), Ok(rip_rbp_vaddr)) => {
                        if current_page_table.translate(rbp_vaddr).is_some() && current_page_table.translate(rip_rbp_vaddr).is_some() {
                            let rip = *(rip_rbp as *const usize);
                            if rip == 0 {
                                error!("  {:>#018X}: BEGINNING OF STACK", rbp);
                                break;
                            }
                            let sec = addr_to_symbol(VirtualAddress::new_canonical(rip));
                            let (symbol_name, offset) = sec.unwrap_or_else(|| (String::from("??"), 0));
                            error!("  {:>#018X}: {:>#018X} in {} + {:#X}", rbp, rip, symbol_name, offset);
                            rbp = *(rbp as *const usize);
                        } else {
                            error!("  {:>#018X}: GUARD PAGE", rbp);
                            break;
                        }
                    }
                    _ => {
                        error!(" {:>#018X}: INVALID VIRTUAL ADDRESS", rbp);
                        break;
                    }
                }
                
            } else {
                error!("  {:>#018X}: RBP OVERFLOW", rbp);
            }
        }
    }
}
