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

extern crate alloc;
#[macro_use] extern crate log;
#[macro_use] extern crate vga_buffer;
extern crate memory;
extern crate panic_wrapper;
extern crate mod_mgmt;
extern crate task;
extern crate gimli;
extern crate fallible_iterator;

mod registers;
mod unwind;

use core::panic::PanicInfo;
use memory::VirtualAddress;
use unwind::{NamespaceUnwinder, Unwinder};
use fallible_iterator::FallibleIterator;
use gimli::BaseAddresses;
use mod_mgmt::{
    metadata::{StrongCrateRef, StrongSectionRef, SectionType},
};




#[cfg(not(test))]
#[lang = "eh_personality"]
#[no_mangle]
#[doc(hidden)]
pub extern "C" fn eh_personality() {
    error!("EH_PERSONALITY IS UNHANDLED!");
}


/// The singular entry point for a language-level panic.
#[cfg(not(test))]
#[panic_handler] // same as:  #[lang = "panic_impl"]
#[doc(hidden)]
fn panic_unwind_test(info: &PanicInfo) -> ! {
    // Stuff for unwinding here
    trace!("panic_unwind_test() [top]: {:?}", info);
    let mut rbp: usize;
    let mut rsp: usize;
    let mut rip: usize;
    unsafe {
        // On x86 you cannot directly read the value of the instruction pointer (RIP),
        // so we use a trick that exploits RIP-relateive addressing to read the current value of RIP (also gets RBP and RSP)
        asm!("lea $0, [rip]" : "=r"(rip), "={rbp}"(rbp), "={rsp}"(rsp) : : "memory" : "intel", "volatile");
    }
    debug!("panic_unwind_test(): register values: RIP: {:#X}, RSP: {:#X}, RBP: {:#X}", rip, rsp, rbp);
    let _curr_instruction_pointer = VirtualAddress::new_canonical(rip);

    let curr_task = task::get_my_current_task().expect("panic_unwind_test(): get_my_current_task() failed");
    let namespace = curr_task.get_namespace();
    let (mmi_ref, app_crate_ref, _is_idle_task) = { 
        let t = curr_task.lock();
        (t.mmi.clone(), t.app_crate.clone(), t.is_an_idle_task)
    };
    // We should ensure that the lock on the curr_task isn't held here,
    // in order to allow the panic handler and other functions below to acquire it. 

    panic_wrapper::stack_trace(
        &mmi_ref.lock().page_table,
        &|instruction_pointer: VirtualAddress| {
            namespace.get_section_containing_address(instruction_pointer, app_crate_ref.as_ref(), false)
                .map(|(sec_ref, offset)| (sec_ref.lock().name.clone(), offset))
        },
    );

    /*
    let starting_instruction_pointer = panic_wrapper::get_first_non_panic_instruction_pointer(
        &mmi_ref.lock().page_table,
        &|instruction_pointer: VirtualAddress| {
            namespace.get_section_containing_address(instruction_pointer, app_crate_ref.as_ref(), false)
                .map(|(sec_ref, offset)| (sec_ref.lock().name.clone(), offset))
        },
    ).expect("couldn't determine which instruction pointer was the first non-panic-related one");
    debug!("panic_unwind_test(): starting search at RIP: {:#X}", starting_instruction_pointer);
    // search for unwind entry related to the current instruction pointer
    // first, we need to get the containing crate for this IP
    let my_crate = namespace.get_crate_containing_address(starting_instruction_pointer, app_crate_ref.as_ref(), false)
        .expect("panic_unwind_test(): couldn't get crate containing address");
    debug!("panic_unwind_test(): looking at crate {:?}", my_crate);
    */

    debug!("Starting NamespaceUnwinder!");
    let mut unwinder = NamespaceUnwinder::new(namespace, app_crate_ref);
    unwinder.trace(print_stack_frames);
    debug!("Done with NamespaceUnwinder!");

    // handle_eh_frame(&namespace, &my_crate, starting_instruction_pointer, true).expect("handle_eh_frame failed");
    


    error!("REACHED END OF panic_unwind_test()! Looping infinitely...");
    loop {}
}


/// Returns a tuple of .eh_frame section for the given `crate_ref`
/// and the base addresses (its .text section address and .eh_frame section address).
/// 
/// # Locking / Deadlock
/// Obtains the lock on the given `crate_ref` 
/// and the lock on all of its sections while iterating through them.
/// 
/// The latter lock on the crate's `rodata_pages` object will be held
/// for the entire lifetime of the returned object. 
fn get_eh_frame_info(crate_ref: &StrongCrateRef) -> Option<(StrongSectionRef, BaseAddresses)> {
    let parent_crate = crate_ref.lock_as_ref();

    let eh_frame_sec_ref = parent_crate.sections.values()
        .filter(|s| s.lock().typ == SectionType::EhFrame)
        .next()?;
    
    let eh_frame_vaddr = eh_frame_sec_ref.lock().start_address().value();
    let text_pages_vaddr = parent_crate.text_pages.as_ref()?.1.start.value();
    let base_addrs = BaseAddresses::default()
        .set_eh_frame(eh_frame_vaddr as u64)
        .set_text(text_pages_vaddr as u64);

    Some((eh_frame_sec_ref.clone(), base_addrs))
}

    // // The map from CIE offset to CIE struct, which is needed to parse every partial FDE
    // let mut cie_map: BTreeMap<usize, CommonInformationEntry<_>> = BTreeMap::new();
    // let mut entries = eh_frame.entries(&base_addrs);
    // while let Some(cfi_entry) = entries.next().map_err(|_e| {
    //     error!("gimli error: {:?}", _e);
    //     "gimli error while iterating through eh_frame entries"
    // })? {
    //     // debug!("Found eh_frame entry: {:?}", cfi_entry);
    //     match cfi_entry {
    //         CieOrFde::Cie(cie) => {
    //             debug!("  --> moving on to CIE at offset {}", cie.offset());
    //             let mut instructions = cie.instructions(&eh_frame, &base_addrs);
    //             while let Some(instr) = instructions.next().map_err(|_e| {
    //                 error!("CIE instructions gimli error: {:?}", _e);
    //                 "gimli error while iterating through eh_frame Cie instructions list"
    //             })? {
    //                 debug!("    CIE instr: {:?}", instr);
    //             }
    //             cie_map.insert(cie.offset(), cie);
    //         }
    //         CieOrFde::Fde(partial_fde) => {
    //             debug!("    Parsing partial FDE...");
    //             let full_fde = partial_fde.parse(|_eh_frame_gimli_sec, _base_addrs, cie_offset| {
    //                 let cie_offset = cie_offset.0;
    //                 debug!("PartialFDE::parse(): cie_offset: {}", cie_offset);
    //                 let required_cie = cie_map.get(&cie_offset).cloned().ok_or_else(|| {
    //                     error!("BUG: partial FDE required CIE offset {:?}, but that CIE couldn't be found. Available CIEs: {:?}", cie_offset, cie_map);
    //                     gimli::Error::NotCiePointer
    //                 });
    //                 required_cie
    //             }).map_err(|_e| {
    //                 error!("gimli error: {:?}", _e);
    //                 "gimli error while parsing partial FDE"
    //             })?;
    //             debug!("      Full FDE: {:?}", full_fde);
    //             let mut instructions = full_fde.instructions(&eh_frame, &base_addrs);
    //             while let Some(instr) = instructions.next().map_err(|_e| {
    //                 error!("FDE instructions gimli error: {:?}", _e);
    //                 "gimli error while iterating through eh_frame FDE instructions list"
    //             })? {
    //                 debug!("    FDE instr: {:?}", instr);
    //             }
    //             let mut uninit_unwind_ctx = UninitializedUnwindContext::new();
    //             let mut table = full_fde.rows(&eh_frame, &base_addrs, &mut uninit_unwind_ctx)
    //                 .map_err(|_e| {
    //                     error!("FDE rows gimli error: {:?}", _e);
    //                     "gimli error while calling fde.rows()"
    //                 })?;
    //             while let Some(row) = table.next_row().map_err(|_e| {
    //                 error!("Table row gimli error: {:?}", _e);
    //                 "gimli error while iterating through FDE unwind table rows"
    //             })? {
    //                 debug!("        FDE unwind row: {:?}", row);
    //             }
    //         }
    //     }
    // }
    // info!("successfully iterated through {}'s eh_frame entries", crate_name);




fn print_stack_frames(stack_frames: &mut unwind::StackFrames) {
    while let Some(frame) = stack_frames.next().expect("stack_frames.next() error") {
        info!("StackFrame: {:#X?}", frame);
        // backtrace::resolve(x.registers()[16].unwrap() as *mut std::os::raw::c_void, |sym| println!("{:?} ({:?}:{:?})", sym.name(), sym.filename(), sym.lineno()));
        // println!("{:?}", frame);
    }
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
    println_raw!("\nOut of Heap Memory! requested allocation: {:?}", _layout);
    error!("Out of Heap Memory! requested allocation: {:?}", _layout);
    loop {}
}