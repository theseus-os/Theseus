// Copyright 2017 Kevin Boos. 
// Licensed under the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>,
// This file may not be copied, modified, or distributed
// except according to those terms.


#![no_std]
#![feature(alloc)]
#![feature(lang_items)]
#![feature(used)]



extern crate alloc;
#[macro_use] extern crate log;
extern crate rlibc; // basic memset/memcpy libc functions
extern crate multiboot2;
extern crate x86_64;
extern crate kernel_config; // our configuration options, just a set of const definitions.
extern crate irq_safety; // for irq-safe locking and interrupt utilities


extern crate logger;
extern crate state_store;
extern crate memory; // the virtual memory subsystem
extern crate frame_buffer;
extern crate frame_buffer_3d;
extern crate mod_mgmt;
extern crate apic;
extern crate exceptions;
extern crate captain;
extern crate panic;
extern crate task;


// see this: https://doc.rust-lang.org/1.22.1/unstable-book/print.html#used
#[link_section = ".pre_init_array"] // "pre_init_array" is a section never removed by --gc-sections
#[used]
#[doc(hidden)]
pub fn nano_core_public_func(val: u8) {
    error!("NANO_CORE_PUBLIC_FUNC: got val {}", val);
}


use core::ops::DerefMut;
use x86_64::structures::idt::LockedIdt;

static EARLY_IDT: LockedIdt = LockedIdt::new();



/// Just like Rust's `try!()` macro, but instead of performing an early return upon an error,
/// it invokes the `shutdown()` function upon an error.
macro_rules! shutdown {
    ($expr:expr) => (match $expr {
        Ok(val) => val,
        Err(err_msg) => {
            $crate::shutdown(err_msg);
        }
    });
    ($expr:expr,) => (try!($expr));
}



fn shutdown(msg: &'static str) -> ! {
    warn!("Theseus is shutting down, msg: {}", msg);

    // TODO: handle shutdowns properly with ACPI commands
    panic!(msg);
}



/// The main entry point into Theseus, that is, the first Rust code that the Theseus kernel runs. 
///
/// This is called from assembly code entry point for Theseus, found in `nano_core/src/boot/arch_x86_64/boot.asm`.
///
/// This function does the following things: 
///
/// * Bootstraps the OS, including [logging](../logger/index.html) and basic [exception handlers](../exceptions/fn.init_early_exceptions.html)
/// * Sets up basic [virtual memory](../memory/fn.init.html)
/// * Initializes the [state_store](../state_store/index.html) module
/// * Finally, calls the Captain module, which initializes and configures the rest of Theseus.
///
#[no_mangle]
pub extern "C" fn nano_core_start(multiboot_information_virtual_address: usize) {
	
	// start the kernel with interrupts disabled
	irq_safety::disable_interrupts();
	
    // first, bring up the logger so we can debug
    shutdown!(logger::init().map_err(|_| "couldn't init logger!"));
    trace!("Logger initialized.");

    // initialize basic exception handlers
    exceptions::init_early_exceptions(&EARLY_IDT);

    // safety-wise, we just have to trust the multiboot address we get from the boot-up asm code
    debug!("multiboot_information_vaddr: {:#X}", multiboot_information_virtual_address);
    let boot_info = unsafe { multiboot2::load(multiboot_information_virtual_address) };

    // init memory management: set up stack with guard page, heap, kernel text/data mappings, etc
    // this consumes boot_info
    let (kernel_mmi_ref, text_mapped_pages, rodata_mapped_pages, data_mapped_pages, identity_mapped_pages) = 
        shutdown!(memory::init(boot_info, apic::broadcast_tlb_shootdown));

    //init frame_buffer
    let rs = frame_buffer::init();
    if rs.is_ok() {
        trace!("frame_buffer initialized.");
    } else {
        debug!("nano_core::nano_core_start: {}", rs.unwrap_err());
    }
    let rs = frame_buffer_3d::init();
    if rs.is_ok() {
        trace!("frame_buffer initialized.");
    } else {
        debug!("nano_core::nano_core_start: {}", rs.unwrap_err());
    }

    // now that we have a heap, we can create basic things like state_store
    state_store::init();
    trace!("state_store initialized.");
    
    
    #[cfg(feature = "mirror_serial")]
    {
         // enables mirroring of serial port logging outputs to VGA buffer (for real hardware)
        logger::mirror_to_vga(captain::mirror_to_vga_cb);
    }

    // parse the nano_core crate (the code we're already running) in both regular mode and loadable mode,
    // since we need it to load and run applications as crates in the kernel
    {
        let _num_nano_core_syms = try_exit!(mod_mgmt::parse_nano_core(kernel_mmi_ref.lock().deref_mut(), text_mapped_pages, rodata_mapped_pages, data_mapped_pages, false));
        // debug!("========================== Symbol map after __k_nano_core {}: ========================\n{}", _num_nano_core_syms, mod_mgmt::metadata::dump_symbol_map());
    }
    

    // parse the core library (Rust no_std lib) and the captain crate
    #[cfg(feature = "loadable")] 
    {
        let _num_libcore_syms = try_exit!(mod_mgmt::load_kernel_crate(
            try_exit!(memory::get_module("__k_core").ok_or("couldn't find __k_core module")), 
            kernel_mmi_ref.lock().deref_mut(), false)
        );
        // debug!("========================== Symbol map after nano_core {} and libcore {}: ========================\n{}", _num_nano_core_syms, _num_libcore_syms, mod_mgmt::metadata::dump_symbol_map());
        
        let _num_captain_syms = try_exit!(mod_mgmt::load_kernel_crate(
            try_exit!(memory::get_module("__k_captain").ok_or("couldn't find __k_captain module")), 
            kernel_mmi_ref.lock().deref_mut(), false)
        );
    }


    // at this point, we load and jump directly to the Captain, which will take it from here. 
    // That's it, the nano_core is done! That's really all it does! 
    #[cfg(feature = "loadable")]
    {
        use memory::{MappedPages, MemoryManagementInfo};
        use alloc::arc::Arc;
        use alloc::Vec;
        use irq_safety::MutexIrqSafe;

        let vaddr = shutdown!(mod_mgmt::metadata::get_symbol("captain::init").upgrade().ok_or("no symbol: captain::init")).virt_addr();
        let func: fn(Arc<MutexIrqSafe<MemoryManagementInfo>>, Vec<MappedPages>, usize, usize, usize, usize) -> Result<(), &'static str> = unsafe { ::core::mem::transmute(vaddr) };
        shutdown!(
            func(kernel_mmi_ref, identity_mapped_pages, 
                get_bsp_stack_bottom(), get_bsp_stack_top(),
                get_ap_start_realmode_begin(), get_ap_start_realmode_end()
            )
        );
    }
    #[cfg(not(feature = "loadable"))]
    {
        shutdown!(
            captain::init(kernel_mmi_ref, identity_mapped_pages, 
                get_bsp_stack_bottom(), get_bsp_stack_top(),
                get_ap_start_realmode_begin(), get_ap_start_realmode_end()
            )
        );
    }


    // the captain shouldn't return ...
    shutdown!(Err("captain::init returned unexpectedly... it should be an infinite loop (diverging function)"));
}





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
    
    if let Err(e) = default_panic_handler(fmt_args, file, line, col) {
        error!("PANIC: error in default panic handler: {}.\nPanic in {}:{}:{} -- {}", e, file, line, col, fmt_args);
    }

    // if we failed to handle the panic, there's not really much we can do about it
    // other than just let the thread spin endlessly (which doesn't hurt correctness but is inefficient). 
    // But in general, the thread should be killed by the default panic handler, so it shouldn't reach here.
    
    loop {}
}


/// performs the standard panic handling routine, which involves the following:
/// 
/// * printing a basic panic message
/// * getting the current task 
fn default_panic_handler(fmt_args: core::fmt::Arguments, file: &'static str, line: u32, col: u32) -> Result<(), &'static str> {
    use alloc::String;
    use panic::{PanicInfo, PanicLocation};
    use task::{TaskRef, KillReason};

    let apic_id = apic::get_my_apic_id();
    let panic_info = PanicInfo::with_fmt_args(PanicLocation::new(file, line, col), fmt_args);

    // get current task to see if it has a panic_handler
    let curr_task: Option<&TaskRef> = {
        #[cfg(feature = "loadable")]
        {
            let section = mod_mgmt::metadata::get_symbol("task::get_my_current_task").upgrade().ok_or("no symbol: task::get_my_current_task")?;
            let mut space = 0;
            let func: & fn() -> Option<&'static TaskRef> =
                section.mapped_pages()
                .ok_or("Couldn't get section's mapped_pages for \"task::get_my_current_task\"")?
                .as_func(section.mapped_pages_offset(), &mut space)?; 
            func()
        } 
        #[cfg(not(feature = "loadable"))] 
        {
            task::get_my_current_task()
        }
    };

    let curr_task_name = curr_task.map(|t| t.read().name.clone()).unwrap_or(String::from("UNKNOWN!"));
    let curr_task = curr_task.ok_or("get_my_current_task() failed")?;
    
    // call this task's panic handler, if it has one. 
    let panic_handler = { 
        curr_task.write().take_panic_handler()
    };
    if let Some(ref ph_func) = panic_handler {
        ph_func(&panic_info);
        error!("PANIC handled in task \"{}\" on core {:?} at {} -- {}", curr_task_name, apic_id, panic_info.location, panic_info.msg);
    }
    else {
        error!("PANIC was unhandled in task \"{}\" on core {:?} at {} -- {}", curr_task_name, apic_id, panic_info.location, panic_info.msg);
        memory::stack_trace();
    }

    // kill the offending task (the current task)
    error!("Killing panicked task \"{}\"", curr_task_name);
    curr_task.write().kill(KillReason::Panic(panic_info));
    
    // scheduler::schedule(); // yield the current task after it's done

    Ok(())
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


// symbols exposed in the initial assembly boot files
// DO NOT DEREFERENCE THESE DIRECTLY!! THEY ARE SIMPLY ADDRESSES USED FOR SIZE CALCULATIONS.
// A DIRECT ACCESS WILL CAUSE A PAGE FAULT
extern {
    static initial_bsp_stack_top: usize;
    static initial_bsp_stack_bottom: usize;
    static ap_start_realmode: usize;
    static ap_start_realmode_end: usize;
}

use kernel_config::memory::KERNEL_OFFSET;
/// Returns the starting virtual address of where the ap_start realmode code is.
fn get_ap_start_realmode_begin() -> usize {
    let addr = unsafe { &ap_start_realmode as *const _ as usize };
    // debug!("ap_start_realmode addr: {:#x}", addr);
    addr + KERNEL_OFFSET
}

/// Returns the ending virtual address of where the ap_start realmode code is.
fn get_ap_start_realmode_end() -> usize {
    let addr = unsafe { &ap_start_realmode_end as *const _ as usize };
    // debug!("ap_start_realmode_end addr: {:#x}", addr);
    addr + KERNEL_OFFSET
} 

fn get_bsp_stack_bottom() -> usize {
    unsafe {
        &initial_bsp_stack_bottom as *const _ as usize
    }
}

fn get_bsp_stack_top() -> usize {
    unsafe {
        &initial_bsp_stack_top as *const _ as usize
    }
}
