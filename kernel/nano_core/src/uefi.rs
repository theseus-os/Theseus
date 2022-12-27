use crate::{early_setup, nano_core, try_exit};
use boot_info::uefi::STACK_SIZE;
use bootloader_api::{config::Mapping, BootloaderConfig};
use core::arch::asm;
use memory::VirtualAddress;

#[used]
#[link_section = ".bootloader-config"]
pub static __BOOTLOADER_CONFIG: [u8; BootloaderConfig::SERIALIZED_LEN] = {
    let mut config = BootloaderConfig::new_default();
    config.mappings.physical_memory = Some(Mapping::Dynamic);
    config.mappings.page_table_recursive =
        Some(Mapping::FixedAddress(0o177777_776_000_000_000_0000));
    config.kernel_stack_size = STACK_SIZE as u64;
    config.serialize()
};

#[naked]
#[no_mangle]
#[link_section = ".init.text"]
pub extern "C" fn _start(boot_info: &'static bootloader_api::BootInfo) {
    unsafe {
        asm!(
            // First argument  (rdi): a reference to the boot info (passed through).
            // Second argument (rsi): the top of the double fault handler stack.
            "mov rsi, rsp",
            // Set the kernel stack pointer to the page before the double fault stack.
            //
            // +------------+--------------+--------------------+
            // | guard page | kernel stack | double fault stack |
            // +------------+--------------+--------------------+
            // ^                           ^                    ^
            // |                           |                   rsi (double_fault_stack)
            // kernel_stack_start         rsp
            // 
            //
            // where the guard page and double fault stack are both one page, and the kernel stack is
            // KERNEL_STACK_SIZE_IN_PAGES pages.
            //
            // NOTE: Stacks grow downwards e.g. the kernel stack pointer will grow towards the guard
            // page.
            "sub rsp, 4096",
            "call {}",
            "jmp KEXIT",
            sym rust_entry,
            options(noreturn),
        )
    };
}

fn rust_entry(
    boot_info: &'static bootloader_api::BootInfo,
    double_fault_stack: usize,
) {
    try_exit!(early_setup(double_fault_stack));
    // See the above diagram in `_start`.
    let kernel_stack_start = VirtualAddress::new_canonical(double_fault_stack - STACK_SIZE);
    try_exit!(nano_core(boot_info, kernel_stack_start));
}

#[naked]
#[no_mangle]
#[link_section = ".init.text"]
pub extern "C" fn _error() {
    unsafe {
        asm!(
            // "2:" is a label.
            // See <https://doc.rust-lang.org/nightly/rust-by-example/unsafe/asm.html#labels>
            "2:",
            "hlt",
            "jmp 2b",
            options(noreturn)
        )
    }
}
