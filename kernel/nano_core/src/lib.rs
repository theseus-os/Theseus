// FIXME: Redocument

#![no_std]

#[cfg(all(feature = "bios", feature = "uefi"))]
compile_error!("either the bios or uefi features must be enabled, not both");

#[cfg(all(not(feature = "bios"), not(feature = "uefi")))]
compile_error!("either the bios or uefi features must be enabled");

// TODO: Remove captain extern crate after implementing nano_core. Currently it
// provides the global allocator.
extern crate captain;
extern crate panic_entry;

mod util;

use memory::VirtualAddress;

cfg_if::cfg_if! {
    if #[cfg(feature = "uefi")] {
        #[no_mangle]
        pub extern "C" fn rust_entry(boot_info: &'static mut bootloader_api::BootInfo, stack: usize) {
            try_exit!(early_setup(stack));
            try_exit!(nano_core(boot_info));
        }

        impl BootInformation for bootloader_api::BootInfo {}
    } else if #[cfg(feature = "bios")] {
        #[no_mangle]
        pub extern "C" fn rust_entry(boot_info: usize, stack: usize) {
            try_exit!(early_setup(stack));

            if VirtualAddress::new(boot_info).is_none() {
                util::shutdown(format_args!("multiboot2 info address invalid"));
            }
            let boot_info = match unsafe { multiboot2::load(boot_info) } {
                Ok(i) => i,
                Err(e) => util::shutdown(format_args!("failed to load multiboot 2 info: {e:?}")),
            };

            try_exit!(nano_core(&boot_info));
        }

        impl BootInformation for multiboot2::BootInformation {}
    }
}

trait BootInformation {}

fn early_setup(stack: usize) -> Result<(), &'static str> {
    irq_safety::disable_interrupts();

    let logger_ports = [serial_port_basic::take_serial_port(
        serial_port_basic::SerialPortAddress::COM1,
    )];
    logger::early_init(None, IntoIterator::into_iter(logger_ports).flatten())
        .map_err(|_| "failed to initialise early logging")?;

    exceptions_early::init(Some(VirtualAddress::new_canonical(stack)));

    Ok(())
}

fn nano_core<T>(_boot_info: &T) -> Result<(), &'static str>
where
    T: BootInformation,
{
    loop {}
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

// Used to obtain information about this build of Theseus.
mod build_info {
    include!(concat!(env!("OUT_DIR"), "/built.rs"));
}
