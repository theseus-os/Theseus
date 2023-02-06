use crate::{early_setup, nano_core, shutdown, try_exit};
use boot_info::BootInformation;
use memory::VirtualAddress;

#[no_mangle]
pub extern "C" fn rust_entry(boot_info: usize, double_fault_stack: usize) {
    try_exit!(early_setup(double_fault_stack));
    if VirtualAddress::new(boot_info).is_none() {
        shutdown(format_args!("multiboot2 info address invalid"));
    }
    let boot_info = match unsafe { multiboot2::load(boot_info) } {
        Ok(i) => i,
        Err(e) => shutdown(format_args!("failed to load multiboot 2 info: {e:?}")),
    };
    let kernel_stack_start = try_exit!(
        VirtualAddress::new(double_fault_stack - try_exit!(boot_info.stack_size()))
            .ok_or("invalid kernel stack start")
    );
    try_exit!(nano_core(boot_info, kernel_stack_start));
}
