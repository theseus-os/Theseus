//! The aptly-named tiny crate containing the first OS code to run.
//! 
//! The `nano_core` is very simple, and only does the following things:
//! 
//! 1. Bootstraps the OS after the bootloader is finished, and initializes simple things like logging.
//! 2. Establishes a simple virtual memory subsystem so that other modules can be loaded.
//! 3. Loads the core library module, the `captain` module, and then calls [`captain::init()`](../captain/fn.init.html) as a final step.
//! 4. That's it! Once `nano_core` gives complete control to the `captain`, it takes no other actions.
//!
//! In general, you shouldn't ever need to change `nano_core`. 
//! That's because `nano_core` doesn't contain any specific program logic, 
//! it just sets up an initial environment so that other subsystems can run.
//! 
//! If you want to change how the OS starts up and which systems it initializes, 
//! you should change the code in the [`captain`](../captain/index.html) crate instead.
//! 

#![no_std]
#![feature(alloc)]
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
extern crate exceptions_early;
extern crate captain;
extern crate panic_unwind; // the panic/unwind lang items

#[macro_use] extern crate vga_buffer;


// see this: https://doc.rust-lang.org/1.22.1/unstable-book/print.html#used
#[link_section = ".pre_init_array"] // "pre_init_array" is a section never removed by --gc-sections
#[used]
#[doc(hidden)]
pub fn nano_core_public_func(val: u8) {
    error!("NANO_CORE_PUBLIC_FUNC: got val {}", val);
}


use core::ops::DerefMut;
use x86_64::structures::idt::LockedIdt;


/// An initial interrupt descriptor table for catching very simple exceptions only.
/// This is no longer used after interrupts are set up properly, it's just a failsafe.
static EARLY_IDT: LockedIdt = LockedIdt::new();


/// Just like Rust's `try!()` macro, but instead of performing an early return upon an error,
/// it invokes the `shutdown()` function upon an error in order to cleanly exit Theseus OS.
macro_rules! try_exit {
    ($expr:expr) => (match $expr {
        Ok(val) => val,
        Err(err_msg) => {
            $crate::shutdown(format_args!("{}", err_msg));
        }
    });
    ($expr:expr,) => (try!($expr));
}


/// Shuts down Theseus and prints the given formatted arguuments.
fn shutdown(msg: core::fmt::Arguments) -> ! {
    warn!("Theseus is shutting down, msg: {}", msg);

    // TODO: handle shutdowns properly with ACPI commands
    panic!("{}", msg);
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
    let (kernel_mmi_ref, text_mapped_pages, rodata_mapped_pages, data_mapped_pages, identity_mapped_pages) = try_exit!(memory::init(boot_info));
    println_raw!("nano_core_start(): initialized memory subsystem."); 

    // now that we have virtual memory, including a heap, we can create basic things like state_store
    state_store::init();
    trace!("state_store initialized.");
    println_raw!("nano_core_start(): initialized state store.");     

    // Parse the nano_core crate (the code we're already running) in both regular mode and loadable mode,
    // since we need it to load and run applications as crates in the kernel.
    println_raw!("nano_core_start(): parsing nano_core crate, please wait ..."); 
    match mod_mgmt::parse_nano_core::parse_nano_core(kernel_mmi_ref.lock().deref_mut(), text_mapped_pages, rodata_mapped_pages, data_mapped_pages, false) {
        Ok(_new_syms) => {
            // debug!("========================== Symbol map after k#nano_core {}: ========================\n{}", _new_syms, mod_mgmt::metadata::dump_symbol_map());
        }
        Err((msg, mapped_pages)) => {
            // Because this function takes ownership of the text/rodata/data mapped_pages that cover the currently-running code,
            // we have to make sure these mapped_pages aren't dropped.
            core::mem::forget(mapped_pages);
            shutdown(format_args!("parse_nano_core() failed! error: {}", msg));
        }
    }
    
    // if in loadable mode, parse the two crates we always need: the core library (Rust no_std lib) and the captain
    #[cfg(feature = "loadable")] 
    {
        let core_module = try_exit!(memory::get_module("k#core").ok_or("couldn't find k#core module"));
        let _num_libcore_syms = try_exit!(mod_mgmt::get_default_namespace().load_kernel_crate(core_module, None, kernel_mmi_ref.lock().deref_mut(), false));
        // debug!("========================== Symbol map after nano_core {} and libcore {}: ========================\n{}", _num_nano_core_syms, _num_libcore_syms, mod_mgmt::metadata::dump_symbol_map());
        
        let captain_module = try_exit!(memory::get_module("k#captain").ok_or("couldn't find k#captain module"));
        let _num_captain_syms = try_exit!(mod_mgmt::get_default_namespace().load_kernel_crate(captain_module, None, kernel_mmi_ref.lock().deref_mut(), false));
    }


    // at this point, we load and jump directly to the Captain, which will take it from here. 
    // That's it, the nano_core is done! That's really all it does! 
    #[cfg(feature = "loadable")]
    {
        use alloc::Vec;
        use alloc::arc::Arc;
        use irq_safety::MutexIrqSafe;
        use memory::{MappedPages, MemoryManagementInfo};
        use mod_mgmt::metadata::CrateType;

        let section_ref = try_exit!(
            mod_mgmt::get_default_namespace().get_symbol_or_load("captain::init", CrateType::Kernel.prefix(), None, kernel_mmi_ref.lock().deref_mut(), false)
            .upgrade()
            .ok_or("no symbol: captain::init")
        );
        type CaptainInitFunc = fn(Arc<MutexIrqSafe<MemoryManagementInfo>>, Vec<MappedPages>, usize, usize, usize, usize) -> Result<(), &'static str>;
        let mut space = 0;
        let (mapped_pages, mapped_pages_offset) = { 
            let section = section_ref.lock();
            (section.mapped_pages.clone(), section.mapped_pages_offset)
        };
        let func: &CaptainInitFunc = {
            try_exit!(mapped_pages.lock().as_func(mapped_pages_offset, &mut space))
        };

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
