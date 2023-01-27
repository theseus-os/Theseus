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
#![no_main]
#![feature(naked_functions)]

extern crate panic_entry;

use core::ops::DerefMut;
use memory::VirtualAddress;
use kernel_config::memory::KERNEL_OFFSET;
use vga_buffer::println_raw;

cfg_if::cfg_if! {
    if #[cfg(feature = "uefi")] {
        mod uefi;
    } else if #[cfg(feature = "bios")] {
        mod bios;
    } else {
        compile_error!("either the 'bios' or 'uefi' feature must be enabled");
    }
}

/// Used to obtain information about this build of Theseus.
mod build_info {
    include!(concat!(env!("OUT_DIR"), "/built.rs"));
}

/// Just like Rust's `try!()` macro, but instead of performing an early return
/// upon an error, it invokes the `shutdown()` function upon an error in order
/// to cleanly exit Theseus OS.
#[macro_export]
macro_rules! try_exit {
    ($expr:expr) => {
        match $expr {
            Ok(val) => val,
            Err(err_msg) => {
                $crate::shutdown(format_args!("{}", err_msg));
            }
        }
    };
}

/// Shuts down Theseus and prints the given formatted arguuments.
fn shutdown(msg: core::fmt::Arguments) -> ! {
    println_raw!("Theseus is shutting down, msg: {}", msg);
    log::error!("Theseus is shutting down, msg: {}", msg);

    // TODO: handle shutdowns properly with ACPI commands
    panic!("{}", msg);
}

/// Early setup that must be done prior to loading the boot information.
///
/// This involves:
/// 1. Setting up logging
/// 2. Dumping basic information about the Theseus build
/// 3. Initialising early exceptions
fn early_setup(early_double_fault_stack_top: usize) -> Result<(), &'static str> {
    irq_safety::disable_interrupts();
    println_raw!("Entered early_setup(). Interrupts disabled.");

    let logger_ports = [serial_port_basic::take_serial_port(
        serial_port_basic::SerialPortAddress::COM1,
    )];
    logger::early_init(None, IntoIterator::into_iter(logger_ports).flatten())
        .map_err(|_| "failed to initialise early logging")?;
    log::info!("initialised early logging");
    println_raw!("early_setup(): initialized logger.");

    // Dump basic information about this build of Theseus.
    log::info!("\n    \
        ===================== Theseus build info: =====================\n    \
        CUSTOM CFGs: {} \n    \
        ===============================================================",
        build_info::CUSTOM_CFG_STR,
    );

    exceptions_early::init(Some(VirtualAddress::new_canonical(early_double_fault_stack_top)));
    println_raw!("early_setup(): initialized early IDT with exception handlers.");

    Ok(())
}

/// The nano core routine. See crate-level documentation for more information.
fn nano_core<T>(boot_info: T, kernel_stack_start: VirtualAddress) -> Result<(), &'static str>
where
    T: boot_info::BootInformation
{
    let rsdp_address = boot_info.rsdp();
    println_raw!("nano_core(): bootloader-provided RSDP address: {:X?}", rsdp_address);

    // init memory management: set up stack with guard page, heap, kernel text/data mappings, etc
    let (
        kernel_mmi_ref,
        text_mapped_pages,
        rodata_mapped_pages,
        data_mapped_pages,
        stack,
        bootloader_modules,
        identity_mapped_pages
    ) = memory_initialization::init_memory_management(boot_info, kernel_stack_start)?;
    println_raw!("nano_core(): initialized memory subsystem.");

    state_store::init();
    log::trace!("state_store initialized.");
    println_raw!("nano_core(): initialized state store.");

    // initialize the module management subsystem, so we can create the default crate namespace
    let default_namespace = mod_mgmt::init(bootloader_modules, kernel_mmi_ref.lock().deref_mut())?;
    println_raw!("nano_core(): initialized crate namespace subsystem.");

    // Parse the nano_core crate (the code we're already running) since we need it to load and run applications.
    println_raw!("nano_core(): parsing nano_core crate, please wait ...");
    let (nano_core_crate_ref, ap_realmode_begin, ap_realmode_end, ap_gdt) = match mod_mgmt::parse_nano_core::parse_nano_core(
        default_namespace,
        text_mapped_pages.into_inner(),
        rodata_mapped_pages.into_inner(),
        data_mapped_pages.into_inner(),
        false,
    ) {
        Ok((nano_core_crate_ref, init_symbols, _num_new_syms)) => {
            // Get symbols from the boot assembly code that defines where the ap_start code are.
            // They will be present in the ".init" sections, i.e., in the `init_symbols` list. 
            let ap_realmode_begin = init_symbols
                .get("ap_start_realmode")
                .and_then(|v| VirtualAddress::new(*v + KERNEL_OFFSET))
                .ok_or("Missing/invalid symbol expected from assembly code \"ap_start_realmode\"")?;
            let ap_realmode_end = init_symbols
                .get("ap_start_realmode_end")
                .and_then(|v| VirtualAddress::new(*v + KERNEL_OFFSET))
                .ok_or("Missing/invalid symbol expected from assembly code \"ap_start_realmode_end\"")?;

            let ap_gdt = {
                let mut ap_gdt_virtual_address = None;
                for (_, section) in nano_core_crate_ref.lock_as_ref().sections.iter() {
                    if section.name == "GDT_AP".into() {
                        ap_gdt_virtual_address = Some(section.virt_addr);
                        break;
                    }
                }

                // The identity-mapped virtual address of GDT_AP.
                VirtualAddress::new(
                    memory::translate(ap_gdt_virtual_address.ok_or(
                        "Missing/invalid symbol expected from data section \"GDT_AP\"",
                    )?)
                    .ok_or("Failed to translate \"GDT_AP\"")?
                    .value(),
                )
                .ok_or("couldn't convert \"GDT_AP\" physical address to virtual")?
            };
            // debug!("ap_realmode_begin: {:#X}, ap_realmode_end: {:#X}", ap_realmode_begin, ap_realmode_end);
            (nano_core_crate_ref, ap_realmode_begin, ap_realmode_end, ap_gdt)
        }
        Err((msg, mapped_pages_array)) => {
            // Because this function takes ownership of the text/rodata/data mapped_pages that cover the currently-running code,
            // we have to make sure these mapped_pages aren't dropped.
            core::mem::forget(mapped_pages_array);
            return Err(msg);
        }
    };
    println_raw!("nano_core(): finished parsing the nano_core crate.");

    #[cfg(loadable)] {
        // This isn't currently necessary; we can always add it in back later if/when needed.
        // // If in loadable mode, load each of the nano_core's constituent crates such that other crates loaded in the future
        // // can depend on those dynamically-loaded instances rather than on the statically-linked sections in the nano_core's base kernel image.
        // try_exit!(mod_mgmt::replace_nano_core_crates::replace_nano_core_crates(&default_namespace, nano_core_crate_ref, &kernel_mmi_ref));
    }
    drop(nano_core_crate_ref);
    
    // if in loadable mode, parse the crates we always need: the core library (Rust no_std lib), the panic handlers, and the captain
    #[cfg(loadable)] {
        use mod_mgmt::CrateNamespace;
        println_raw!("nano_core(): loading the \"captain\" crate...");
        let (captain_file, _ns) = CrateNamespace::get_crate_object_file_starting_with(default_namespace, "captain-").ok_or("couldn't find the singular \"captain\" crate object file")?;
        let (_captain_crate, _num_captain_syms) = default_namespace.load_crate(&captain_file, None, &kernel_mmi_ref, false)?;
        println_raw!("nano_core(): loading the panic handling crate(s)...");
        let (panic_wrapper_file, _ns) = CrateNamespace::get_crate_object_file_starting_with(default_namespace, "panic_wrapper-").ok_or("couldn't find the singular \"panic_wrapper\" crate object file")?;
        let (_pw_crate, _num_pw_syms) = default_namespace.load_crate(&panic_wrapper_file, None, &kernel_mmi_ref, false)?;
    }


    // at this point, we load and jump directly to the Captain, which will take it from here. 
    // That's it, the nano_core is done! That's really all it does! 
    println_raw!("nano_core(): invoking the captain...");
    #[cfg(not(loadable))] {
        captain::init(kernel_mmi_ref, identity_mapped_pages, stack, ap_realmode_begin, ap_realmode_end, ap_gdt, rsdp_address)?;
    }
    #[cfg(loadable)] {
        use memory::{EarlyIdentityMappedPages, MmiRef, PhysicalAddress};
        use no_drop::NoDrop;
        use stack::Stack;

        let section = default_namespace
            .get_symbol_starting_with("captain::init::")
            .upgrade()
            .ok_or("no single symbol matching \"captain::init\"")?;
        log::info!("The nano_core (in loadable mode) is invoking the captain init function: {:?}", section.name);

        type CaptainInitFunc = fn(MmiRef, NoDrop<EarlyIdentityMappedPages>, NoDrop<Stack>, VirtualAddress, VirtualAddress, VirtualAddress, Option<PhysicalAddress>) -> Result<(), &'static str>;
        let func: &CaptainInitFunc = unsafe { section.as_func() }?;

        func(kernel_mmi_ref, identity_mapped_pages, stack, ap_realmode_begin, ap_realmode_end, ap_gdt, rsdp_address)?;
    }

    // the captain shouldn't return ...
    Err("captain::init returned unexpectedly... it should be an infinite loop (diverging function)")
}



// These extern definitions are here just to ensure that these symbols are defined in the assembly files. 
// Defining them here produces a linker error if they are absent, which is better than a runtime error (early detection!).
// We don't actually use them, and they should not be accessed or dereferenced, because they are merely values, not addresses. 
#[allow(dead_code)]
extern {
    static initial_bsp_stack_guard_page: usize;
    static initial_bsp_stack_bottom: usize;
    static initial_bsp_stack_top: usize;
    static ap_start_realmode: usize;
    static ap_start_realmode_end: usize;
}


/// This module is a hack to get around the issue of no_mangle symbols
/// not being exported properly from the `libm` crate in no_std environments.
mod libm;

/// Implements OS support for GCC's stack smashing protection.
/// This isn't used at the moment, but we make it available in case 
/// any foreign code (e.g., C code) wishes to use it.
/// 
/// You can disable the need for this via the `-fno-stack-protection` GCC option.
mod stack_smash_protection;
