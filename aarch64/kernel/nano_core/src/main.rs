#![no_std]
#![no_main]

use core::arch::asm;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    unsafe { asm!("b {}", sym _start, options(noreturn)) };
    // loop {
    //     unsafe { asm!("mov x1, #0xbeef") };
    // }
}

#[panic_handler]
fn panic_handler(_: &core::panic::PanicInfo) -> ! {
    loop {
        unsafe { asm!("mov x1, #0xc0de") };
    }
}
