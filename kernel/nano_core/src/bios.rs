//! The main entry point into Rust code from a legacy BIOS (multiboot2) bootloader.

use crate::{nano_core, shutdown, try_exit};
use boot_info::BootInformation;
use memory::VirtualAddress;

#[no_mangle]
pub extern "C" fn rust_entry(boot_info_vaddr: usize, double_fault_stack_top: usize) {
    if VirtualAddress::new(boot_info_vaddr).is_none() {
        shutdown(format_args!("BUG: multiboot2 info virtual address is invalid"));
    }
    let boot_info = match unsafe { multiboot2::load(boot_info_vaddr) } {
        Ok(i) => i,
        Err(e) => shutdown(format_args!("BUG: failed to load multiboot 2 info: {e:?}")),
    };
    let kernel_stack_start = try_exit!(
        VirtualAddress::new(double_fault_stack_top - try_exit!(boot_info.stack_size()))
            .ok_or("BUG: kernel_stack_start virtual address is invalid")
    );
    let double_fault_stack_top = try_exit!(
        VirtualAddress::new(double_fault_stack_top)
            .ok_or("BUG: double_fault_stack_top virtual address is invalid")
    );
    try_exit!(nano_core(boot_info, double_fault_stack_top, kernel_stack_start));
}
