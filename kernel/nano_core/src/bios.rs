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
