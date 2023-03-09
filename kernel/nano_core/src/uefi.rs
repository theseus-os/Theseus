use crate::{early_setup, nano_core, try_exit};
use boot_info::uefi::STACK_SIZE;
use core::arch::asm;
use memory::VirtualAddress;
use uefi_bootloader_api::BootInformation;

/// This is effectively a trampoline function that sets up the proper
/// argument values in the proper registers before calling `rust_entry`.
#[naked]
#[no_mangle]
#[link_section = ".init.text"]
pub extern "C" fn _start(boot_info: &'static BootInformation) {
    // Upon entering this function:
    // * The first argument is a reference to the boot info.
    //
    //    --+-----------+--
    //      | boot_info |
    //    --+-----------+--
    //      ^             
    //      |             
    //  1st argument
    //
    // * The second argument is the top vaddr of the double fault handler stack.
    // * The stack pointer (sp) points to the top of the double fault stack:
    //
    //    --+------------+------------------------------+--------------------+--
    //      | guard page | kernel stack (several pages) | double fault stack |
    //    --+------------+------------------------------+--------------------+--
    //      ^                                           ^                    ^
    //      |                                           |                    |
    //  kernel_stack_start                       sp (top of stack)      2nd argument
    //
    // The guard page and double fault stack are both one page in size;
    // the kernel stack is `KERNEL_STACK_SIZE_IN_PAGES` pages long.
    //
    // `kernel_stack_start` is a variable of the `rust_entry` function.
    //
    // Stacks grow downwards on both x86_64 and aarch64,
    // meaning that the stack pointer will grow towards the guard page.
    // That's why we start it at the top (the highest vaddr).
    unsafe {
        #[cfg(target_arch = "x86_64")]
        asm!(
            // Before invoking `rust_entry`, we need to set up:
            // 1. First arg  (in rdi): a reference to the boot info (just pass it through).
            // 2. Second arg (in rsi): the top vaddr of the double fault handler stack.
            "mov rsi, rsp", // Handle #2 above

            // Now, adjust the stack pointer to the page before the double fault stack,
            // which is the top of the initial kernel stack that was allocated for us.
            "sub rsp, 4096",

            // Now invoke the `rust_entry` function.
            "call {}",

            // Execution should never return to this point. If it does, halt the CPU and loop.
            "jmp KEXIT",

            sym rust_entry,
            options(noreturn),
        );

        #[cfg(target_arch = "aarch64")]
        asm!(
            // Before invoking `rust_entry`, we need to set up:
            // 1. First arg  (in x0): a reference to the boot info (just pass it through).
            // 2. Second arg (in x1): the top vaddr of the double fault handler stack.
            "mov x1, sp", // Handle #2 above

            // Now, adjust the stack pointer to the page before the double fault stack,
            // which is the top of the initial kernel stack that was allocated for us.
            "sub sp, sp, 4096",

            // Now invoke the `rust_entry` function.
            "b {}",

            // Execution can never return to this point.
            sym rust_entry,
            options(noreturn),
        );
    };
}

fn rust_entry(boot_info: &'static BootInformation, double_fault_stack: usize) {
    try_exit!(early_setup(double_fault_stack));
    // See the above memory layout diagram in `_start`.
    let kernel_stack_start = VirtualAddress::new_canonical(double_fault_stack - STACK_SIZE);
    try_exit!(nano_core(boot_info, kernel_stack_start));
}

#[naked]
#[no_mangle]
#[link_section = ".init.text"]
pub extern "C" fn _error() {
    unsafe {
        #[cfg(target_arch = "x86_64")]
        asm!(
            // "2:" is a label.
            // See <https://doc.rust-lang.org/nightly/rust-by-example/unsafe/asm.html#labels>
            "2:",
            "hlt",
            "jmp 2b", // jump backwards ("b") to label "2:".
            options(noreturn)
        );

        #[cfg(target_arch = "aarch64")]
        asm!(
            "2:",
            "wfe",
            "bl 2b",
            options(noreturn)
        );
    }
}
