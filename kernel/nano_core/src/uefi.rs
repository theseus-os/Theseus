use crate::{early_setup, nano_core, try_exit};
use bootloader_api::{config::Mapping, BootloaderConfig};
use core::arch::asm;
use kernel_config::memory::{KERNEL_OFFSET, KERNEL_STACK_SIZE_IN_PAGES, PAGE_SIZE};

#[used]
#[link_section = ".bootloader-config"]
pub static __BOOTLOADER_CONFIG: [u8; BootloaderConfig::SERIALIZED_LEN] = {
    let mut config = BootloaderConfig::new_default();
    config.mappings.physical_memory = Some(Mapping::Dynamic);
    config.mappings.page_table_recursive =
        Some(Mapping::FixedAddress(0o177777_776_000_000_000_0000));
    // + 1 accounts for the initial double fault stack. See comment below for more
    // detail.
    config.kernel_stack_size = ((KERNEL_STACK_SIZE_IN_PAGES + 1) * PAGE_SIZE) as u64;
    config.serialize()
};

#[naked]
#[no_mangle]
#[link_section = ".init.text"]
pub unsafe extern "C" fn _start(boot_info: &'static mut bootloader_api::BootInfo) {
    asm!(
        // First argument: a reference to the boot info (passed through).
        // Second argument: the top of the initial double fault stack.
        // The bootloader gives us KERNEL_STACK_SIZE_IN_PAGES + 1 pages for the stack. We make the
        // top page the initial double fault stack, and the remaining ones the actual kernel stack.
        "mov rsi, rsp",
        "sub rsp, 4096",
        "call rust_entry",
        "jmp KEXIT",
        options(noreturn),
    );
}

#[no_mangle]
pub unsafe extern "C" fn rust_entry(
    boot_info: &'static mut bootloader_api::BootInfo,
    stack: usize,
) {
    try_exit!(early_setup(stack));
    try_exit!(nano_core(boot_info as &'static bootloader_api::BootInfo));
}

#[naked]
#[no_mangle]
#[link_section = ".init.text"]
pub unsafe extern "C" fn _error() {
    asm!("hlt", options(noreturn));
}
