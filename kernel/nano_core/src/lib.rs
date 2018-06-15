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
extern crate spin;
extern crate multiboot2;
extern crate x86_64;
extern crate kernel_config; // our configuration options, just a set of const definitions.
extern crate irq_safety; // for irq-safe locking and interrupt utilities


extern crate logger;
extern crate state_store;
extern crate memory; // the virtual memory subsystem
extern crate mod_mgmt;
extern crate apic;
extern crate exceptions_early;
extern crate captain;
extern crate panic_handling;

#[macro_use] extern crate vga_buffer;


// see this: https://doc.rust-lang.org/1.22.1/unstable-book/print.html#used
#[link_section = ".pre_init_array"] // "pre_init_array" is a section never removed by --gc-sections
#[used]
#[doc(hidden)]
pub fn nano_core_public_func(val: u8) {
    error!("NANO_CORE_PUBLIC_FUNC: got val {}", val);
}


use core::ops::DerefMut;
use alloc::arc::Arc;
use spin::Once;
use x86_64::structures::idt::LockedIdt;
use memory::MappedPages;


/// An initial interrupt descriptor table for catching very simple exceptions only.
/// This is no longer used after interrupts are set up properly, it's just a failsafe.
static EARLY_IDT: LockedIdt = LockedIdt::new();

/// References to the kernel's text, rodata, and data mapped pages,
/// which we hold here because if they were accidentally dropped, the OS would triple fault
/// beacuse they contain the code/data currently being run.
/// These aren't needed for regular proper operation, they're only a failsafe.
static NANO_CORE_PAGES: Once<(Arc<MappedPages>, Arc<MappedPages>, Arc<MappedPages>)> = Once::new();



/// Just like Rust's `try!()` macro, but instead of performing an early return upon an error,
/// it invokes the `shutdown()` function upon an error in order to cleanly exit Theseus OS.
macro_rules! try_exit {
    ($expr:expr) => (match $expr {
        Ok(val) => val,
        Err(err_msg) => {
            $crate::shutdown(err_msg);
        }
    });
    ($expr:expr,) => (try!($expr));
}


/// Shuts down Theseus and prints the given string.
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
/// * Bootstraps the OS, including [logging](../logger/index.html) 
///   and basic early [exception handlers](../exceptions_early/fn.init.html)
/// * Sets up basic [virtual memory](../memory/fn.init.html)
/// * Initializes the [state_store](../state_store/index.html) module
/// * Finally, calls the Captain module, which initializes and configures the rest of Theseus.
///
/// If a failure occurs and is propagated back up to this function, the OS is shut down.
/// 
/// # Note
/// In general, you never need to modify the `nano_core` to change Theseus's behavior,
/// because the `nano_core` is essentially logic-agnostic boilerplate code and set up routines. 
/// If you want to customize or change how the OS components are initialized, 
/// then change the [`captain::init`](../captain/fn.init.html) routine.
/// 
#[no_mangle]
pub extern "C" fn nano_core_start(multiboot_information_virtual_address: usize) {
    println_raw!("Entered nano_core_start()."); 
	
	// start the kernel with interrupts disabled
	irq_safety::disable_interrupts();

    // first, bring up the logger so we can debug
    try_exit!(logger::init().map_err(|_| "couldn't init logger!"));
    trace!("Logger initialized.");
    println_raw!("nano_core_start(): initialized logger."); 

    // initialize basic exception handlers
    exceptions_early::init(&EARLY_IDT);
    println_raw!("nano_core_start(): initialized early IDT with exception handlers."); 

    // safety-wise, we have to trust the multiboot address we get from the boot-up asm code, but we can check its validity
    if !memory::Page::is_valid_address(multiboot_information_virtual_address) {
        try_exit!(Err("multiboot info address was invalid! Ensure that nano_core_start() is being invoked properly from boot.asm!"));
    }
    let boot_info = unsafe { multiboot2::load(multiboot_information_virtual_address) };
    println_raw!("nano_core_start(): loaded multiboot2 info."); 


    // init memory management: set up stack with guard page, heap, kernel text/data mappings, etc
    // this consumes boot_info
    // TODO FIXME:  remove apic as a hard static dependency here. Register the tlb shootdown func elsewhere?
    let (kernel_mmi_ref, text_mapped_pages, rodata_mapped_pages, data_mapped_pages, identity_mapped_pages) = 
        try_exit!(memory::init(boot_info, apic::broadcast_tlb_shootdown));
    println_raw!("nano_core_start(): initialized memory subsystem."); 

    // save references to the kernel's currently running text, rodata, and data sections
    let text_mapped_pages   = Arc::new(text_mapped_pages);
    let rodata_mapped_pages = Arc::new(rodata_mapped_pages);
    let data_mapped_pages   = Arc::new(data_mapped_pages);
    NANO_CORE_PAGES.call_once(|| (text_mapped_pages.clone(), rodata_mapped_pages.clone(), data_mapped_pages.clone()));


    // now that we have virtual memory, including a heap, we can create basic things like state_store
    state_store::init();
    trace!("state_store initialized.");
    println_raw!("nano_core_start(): initialized state store."); 
    

    // parse the nano_core crate (the code we're already running) in both regular mode and loadable mode,
    // since we need it to load and run applications as crates in the kernel
    {
        println_raw!("nano_core_start(): parsing nano_core crate, please wait ..."); 
        let _num_nano_core_syms = try_exit!(mod_mgmt::parse_nano_core::parse_nano_core(kernel_mmi_ref.lock().deref_mut(), text_mapped_pages, rodata_mapped_pages, data_mapped_pages, false));
        // debug!("========================== Symbol map after __k_nano_core {}: ========================\n{}", _num_nano_core_syms, mod_mgmt::metadata::dump_symbol_map());
    }
    
    // if in loadable mode, parse the two crates we always need: the core library (Rust no_std lib) and the captain
    #[cfg(feature = "loadable")] 
    {
        let core_module = try_exit!(memory::get_module("__k_core").ok_or("couldn't find __k_core module"));
        let _num_libcore_syms = try_exit!(mod_mgmt::load_kernel_crate(core_module, kernel_mmi_ref.lock().deref_mut(), false));
        // debug!("========================== Symbol map after nano_core {} and libcore {}: ========================\n{}", _num_nano_core_syms, _num_libcore_syms, mod_mgmt::metadata::dump_symbol_map());
        
        let captain_module = try_exit!(memory::get_module("__k_captain").ok_or("couldn't find __k_captain module"));
        let _num_captain_syms = try_exit!(mod_mgmt::load_kernel_crate(captain_module, kernel_mmi_ref.lock().deref_mut(), false));
    }


    // at this point, we load and jump directly to the Captain, which will take it from here. 
    // That's it, the nano_core is done! That's really all it does! 
    #[cfg(feature = "loadable")]
    {
        use alloc::Vec;
        use alloc::arc::Arc;
        use irq_safety::MutexIrqSafe;
        use memory::{MappedPages, MemoryManagementInfo};

        let section = try_exit!(
            mod_mgmt::metadata::get_symbol_or_load("captain::init", kernel_mmi_ref.lock().deref_mut())
            .upgrade()
            .ok_or("no symbol: captain::init")
        );
        type CaptainInitFunc = fn(Arc<MutexIrqSafe<MemoryManagementInfo>>, Vec<MappedPages>, usize, usize, usize, usize) -> Result<(), &'static str>;
        let mut space = 0;
        let offset = section.lock().mapped_pages_offset;
        let func: &CaptainInitFunc = try_exit!( 
            try_exit!(section.lock().mapped_pages.upgrade().ok_or("Couldn't get section's mapped_pages for \"captain::init\""))
                .as_func(offset, &mut space)
        );

        try_exit!(
            func(kernel_mmi_ref, identity_mapped_pages, 
                get_bsp_stack_bottom(), get_bsp_stack_top(),
                get_ap_start_realmode_begin(), get_ap_start_realmode_end()
            )
        );
    }
    #[cfg(not(feature = "loadable"))]
    {
        try_exit!(
            captain::init(kernel_mmi_ref, identity_mapped_pages, 
                get_bsp_stack_bottom(), get_bsp_stack_top(),
                get_ap_start_realmode_begin(), get_ap_start_realmode_end()
            )
        );
    }


    // the captain shouldn't return ...
    try_exit!(Err("captain::init returned unexpectedly... it should be an infinite loop (diverging function)"));
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
    
    // Since a panic could occur before the memory subsystem is initialized,
    // we must check before using alloc types or other functions that depend on the memory system (the heap).
    // We can check that by seeing if the kernel mmi has been initialized.
    let kernel_mmi_ref = memory::get_kernel_mmi_ref();  
    let res = if kernel_mmi_ref.is_some() {
        // proceed with calling the panic_wrapper, but don't shutdown with try_exit() if errors occur here
        #[cfg(feature = "loadable")]
        {
            type PanicWrapperFunc = fn(fmt_args: core::fmt::Arguments, file: &'static str, line: u32, col: u32) -> Result<(), &'static str>;
            let section = kernel_mmi_ref.and_then(|kernel_mmi| {
                mod_mgmt::metadata::get_symbol_or_load("panic_handling::panic_wrapper", kernel_mmi.lock().deref_mut()).upgrade()
            }).ok_or("Couldn't get symbol: \"panic_handling::panic_wrapper\"");

            // call the panic_wrapper function, otherwise return an Err into "res"
            let mut space = 0;
            section.and_then(|sec| {
                let offset = sec.lock().mapped_pages_offset;
                let mapped_pages = sec.lock().mapped_pages.upgrade().ok_or("Couldn't get mapped_pages for panic_wrapper");
                mapped_pages.and_then(|mp| {
                    mp.as_func::<PanicWrapperFunc>(offset, &mut space)
                        .and_then(|func| func(fmt_args, file, line, col)) // actually call the function
                    }
                )
            }) 
        }
        #[cfg(not(feature = "loadable"))]
        {
            panic_handling::panic_wrapper(fmt_args, file, line, col)
        }
    }
    else {
        Err("memory subsystem not yet initialized, cannot call panic_wrapper because it requires alloc types")
    };

    if let Err(_e) = res {
        // basic early panic printing with no dependencies
        error!("PANIC in {}:{}:{} -- {}", file, line, col, fmt_args);
        println_raw!("\nPANIC in {}:{}:{} -- {}", file, line, col, fmt_args);
    }

    // if we failed to handle the panic, there's not really much we can do about it
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
