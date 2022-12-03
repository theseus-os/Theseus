// FIXME: Redocument

#![no_std]
#![no_main]

// TODO: Remove captain extern crate after implementing nano_core. Currently it
// provides the global allocator.
extern crate captain;
extern crate panic_entry;

mod util;

use core::ops::DerefMut;
use kernel_config::memory::{KERNEL_OFFSET, KERNEL_STACK_SIZE_IN_PAGES, PAGE_SIZE};
use memory::VirtualAddress;

cfg_if::cfg_if! {
    if #[cfg(feature = "uefi")] {
        use bootloader_api::{config::Mapping, BootloaderConfig};

        #[no_mangle]
        pub extern "C" fn rust_entry(_boot_info: &'static mut bootloader_api::BootInfo, stack: usize) {
            try_exit!(early_setup(stack));
            try_exit!(nano_core(_boot_info as &'static bootloader_api::BootInfo));
        }

        #[link_section = ".bootloader-config"]
        #[used]
        pub static __BOOTLOADER_CONFIG: [u8; BootloaderConfig::SERIALIZED_LEN] = {
            // FIXME: Is the default config ok?
            let mut config = BootloaderConfig::new_default();
            config.mappings.physical_memory = Some(Mapping::Dynamic);
            config.mappings.page_table_recursive = Some(Mapping::FixedAddress(0o177777_776_000_000_000_0000));
            config.kernel_stack_size = ((KERNEL_STACK_SIZE_IN_PAGES + 2) * PAGE_SIZE) as u64;
            config.serialize()
        };
    } else if #[cfg(feature = "bios")] {
        use util::shutdown;

        #[no_mangle]
        pub extern "C" fn rust_entry(boot_info: usize, stack: usize) {
            try_exit!(early_setup(stack));
            if VirtualAddress::new(boot_info).is_none() {
                shutdown(format_args!("multiboot2 info address invalid"));
            }
            let boot_info = match unsafe { multiboot2::load(boot_info) } {
                Ok(i) => i,
                Err(e) => shutdown(format_args!("failed to load multiboot 2 info: {e:?}")),
            };

            try_exit!(nano_core(boot_info));
        }
    }
}

fn early_setup(stack: usize) -> Result<(), &'static str> {
    irq_safety::disable_interrupts();

    let logger_ports = [serial_port_basic::take_serial_port(
        serial_port_basic::SerialPortAddress::COM1,
    )];
    logger::early_init(None, IntoIterator::into_iter(logger_ports).flatten())
        .map_err(|_| "failed to initialise early logging")?;
    log::info!("initialised early logging");

    exceptions_early::init(Some(VirtualAddress::new_canonical(stack)));

    Ok(())
}

fn nano_core<T>(boot_info: T) -> Result<(), &'static str>
where
    T: boot_info::BootInformation,
{
    let rsdp = boot_info.rsdp();
    // init memory management: set up stack with guard page, heap, kernel text/data
    // mappings, etc
    let (
        kernel_mmi_ref,
        text_mapped_pages,
        rodata_mapped_pages,
        data_mapped_pages,
        stack,
        bootloader_modules,
        identity_mapped_pages,
    ) = memory_initialization::init_memory_management(boot_info)?;
    println_raw!("nano_core_start(): initialized memory subsystem.");

    state_store::init();
    println_raw!("nano_core_start(): initialized state store.");

    // initialize the module management subsystem, so we can create the default
    // crate namespace
    let default_namespace = mod_mgmt::init(bootloader_modules, kernel_mmi_ref.lock().deref_mut())?;
    println_raw!("nano_core_start(): initialized crate namespace subsystem.");

    // Parse the nano_core crate (the code we're already running) since we need it
    // to load and run applications.
    println_raw!("nano_core_start(): parsing nano_core crate, please wait ...");
    let (nano_core_crate_ref, ap_realmode_begin, ap_realmode_end) =
        match mod_mgmt::parse_nano_core::parse_nano_core(
            default_namespace,
            text_mapped_pages.into_inner(),
            rodata_mapped_pages.into_inner(),
            data_mapped_pages.into_inner(),
            false,
        ) {
            Ok((nano_core_crate_ref, init_symbols, _num_new_syms)) => {
                // Get symbols from the boot assembly code that defines where the ap_start code
                // are. They will be present in the ".init" sections, i.e., in
                // the `init_symbols` list.
                let ap_realmode_begin = init_symbols
                    .get("ap_start_realmode")
                    .and_then(|v| VirtualAddress::new(*v + KERNEL_OFFSET))
                    .ok_or(
                        "Missing/invalid symbol expected from assembly code \"ap_start_realmode\"",
                    )?;
                let ap_realmode_end = init_symbols.get("ap_start_realmode_end")
                    .and_then(|v| VirtualAddress::new(*v + KERNEL_OFFSET))
                    .ok_or("Missing/invalid symbol expected from assembly code \"ap_start_realmode_end\"")?;
                // debug!("ap_realmode_begin: {:#X}, ap_realmode_end: {:#X}", ap_realmode_begin,
                // ap_realmode_end);
                (nano_core_crate_ref, ap_realmode_begin, ap_realmode_end)
            }
            Err((msg, mapped_pages_array)) => {
                // Because this function takes ownership of the text/rodata/data mapped_pages
                // that cover the currently-running code, we have to make sure
                // these mapped_pages aren't dropped.
                // TODO: Is this necessary?
                core::mem::forget(mapped_pages_array);
                return Err(msg);
            }
        };
    println_raw!("nano_core_start(): finished parsing the nano_core crate.");

    #[cfg(loadable)]
    {
        // This isn't currently necessary; we can always add it in back later
        // if/when needed. // If in loadable mode, load each of the
        // nano_core's constituent crates such that other crates loaded in the
        // future // can depend on those dynamically-loaded instances
        // rather than on the statically-linked sections in the nano_core's base
        // kernel image.
        // try_exit!(mod_mgmt::replace_nano_core_crates::replace_nano_core_crates(&default_namespace, nano_core_crate_ref, &kernel_mmi_ref));
    }
    drop(nano_core_crate_ref);

    // if in loadable mode, parse the crates we always need: the core library (Rust
    // no_std lib), the panic handlers, and the captain
    #[cfg(loadable)]
    {
        use mod_mgmt::CrateNamespace;

        println_raw!("nano_core_start(): loading the \"captain\" crate...");
        let (captain_file, _ns) =
            CrateNamespace::get_crate_object_file_starting_with(default_namespace, "captain-")
                .ok_or("couldn't find the singular \"captain\" crate object file")?;
        let (_captain_crate, _num_captain_syms) =
            default_namespace.load_crate(&captain_file, None, &kernel_mmi_ref, false)?;
        println_raw!("nano_core_start(): loading the panic handling crate(s)...");
        let (panic_wrapper_file, _ns) = CrateNamespace::get_crate_object_file_starting_with(
            default_namespace,
            "panic_wrapper-",
        )
        .ok_or("couldn't find the singular \"panic_wrapper\" crate object file")?;
        let (_pw_crate, _num_pw_syms) =
            default_namespace.load_crate(&panic_wrapper_file, None, &kernel_mmi_ref, false)?;
    }

    // at this point, we load and jump directly to the Captain, which will take it
    // from here. That's it, the nano_core is done! That's really all it does!
    println_raw!("nano_core_start(): invoking the captain...");
    #[cfg(not(loadable))]
    {
        captain::init(
            kernel_mmi_ref,
            identity_mapped_pages,
            stack,
            ap_realmode_begin,
            ap_realmode_end,
            rsdp,
        )?;
    }
    #[cfg(loadable)]
    {
        use alloc::vec::Vec;
        use memory::{MappedPages, MmiRef};
        use no_drop::NoDrop;

        let section = try_exit!(default_namespace
            .get_symbol_starting_with("captain::init::")
            .upgrade()
            .ok_or("no single symbol matching \"captain::init\""));
        info!(
            "The nano_core (in loadable mode) is invoking the captain init function: {:?}",
            section.name
        );

        type CaptainInitFunc = fn(
            MmiRef,
            NoDrop<Vec<MappedPages>>,
            NoDrop<stack::Stack>,
            VirtualAddress,
            VirtualAddress,
            Option<usize>,
        ) -> Result<(), &'static str>;
        let func: &CaptainInitFunc = unsafe { section.as_func() }?;

        func(
            kernel_mmi_ref,
            identity_mapped_pages,
            stack,
            ap_realmode_begin,
            ap_realmode_end,
            rsdp,
        )
    }

    // the captain shouldn't return ...
    Err("captain::init returned unexpectedly... it should be an infinite loop (diverging function)")
}

// These extern definitions are here just to ensure that these symbols are
// defined in the assembly files. Defining them here produces a linker error if
// they are absent, which is better than a runtime error (early detection!).
// We don't actually use them, and they should not be accessed or dereferenced,
// because they are merely values, not addresses.
#[allow(dead_code)]
extern "C" {
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
/// You can disable the need for this via the `-fno-stack-protection` GCC
/// option.
mod stack_smash_protection;
