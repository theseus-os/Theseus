//! The main entry point into Rust code from a legacy BIOS (multiboot2) bootloader.

use crate::{nano_core, shutdown};
use boot_info::BootInformation;
use memory::VirtualAddress;

#[no_mangle]
pub extern "C" fn rust_entry(boot_info_vaddr: usize, double_fault_stack_top: usize) {
    match inner(boot_info_vaddr, double_fault_stack_top) {
        Ok(_) => shutdown(format_args!("BUG: nano_core() unexpectedly returned!")),
        Err(e) => shutdown(format_args!("{e}")),
    }
}

fn inner(boot_info_vaddr: usize, double_fault_stack_top: usize) -> Result<(), &'static str> {
    VirtualAddress::new(boot_info_vaddr)
        .ok_or("BUG: multiboot2 info virtual address is invalid")?;
    let boot_info = unsafe { multiboot2::load(boot_info_vaddr) }
        .ok()
        .ok_or("BUG: failed to load multiboot 2 info")?;
    let kernel_stack_start = VirtualAddress::new(double_fault_stack_top - boot_info.stack_size()?)
        .ok_or("BUG: kernel_stack_start virtual address is invalid")?;
    let double_fault_stack_top = VirtualAddress::new(double_fault_stack_top)
        .ok_or("BUG: double_fault_stack_top virtual address is invalid")?;

    nano_core(boot_info, double_fault_stack_top, kernel_stack_start)
}
