#![feature(naked_functions)]
#![feature(abi_efiapi)]
#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec;
use core::fmt::Write;
use core::arch::asm;

use uefi_services::init;
use uefi::prelude::entry;
use uefi::Status;
use uefi::Handle;
use uefi::table::SystemTable;
use uefi::table::Boot;

use pl011_qemu::PL011;
use pl011_qemu::UART1;

pub type Pl011 = PL011<UART1>;

#[inline(never)]
extern "C" fn inf_loop_0xbeef() -> ! {
    unsafe { asm!("mov x1, #0xbeef") };
    loop {}
}

#[entry]
fn main(
    handle: Handle,
    mut system_table: SystemTable<Boot>,
) -> Status {
    init(&mut system_table).unwrap();
    let bootsvc = system_table.boot_services();

    let safety = 16;

    let mmap_size = bootsvc.memory_map_size();
    let mut mmap = vec![0; mmap_size.map_size + safety * mmap_size.entry_size];

    let _ = system_table.exit_boot_services(handle, &mut mmap).unwrap();

    let mut logger = PL011::new(UART1::take().unwrap());

    let _ = logger.write_str("Hello, World!\r\n");

    let _ = logger.write_str("Going to infinite loop now.\r\n");
    inf_loop_0xbeef();
}
