#![feature(naked_functions)]
#![feature(abi_efiapi)]
#![no_std]
#![no_main]

extern crate alloc;
extern crate logger;

use alloc::vec;
use core::arch::asm;

use uefi::prelude::entry;
use uefi::Status;
use uefi::Handle;
use uefi::table::SystemTable;
use uefi::table::Boot;

use log::info;

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
    logger::init();
    info!("Hello, World!");

    uefi_services::init(&mut system_table).unwrap();
    let bootsvc = system_table.boot_services();

    let safety = 16;

    let mmap_size = bootsvc.memory_map_size();
    let mut mmap = vec![0; mmap_size.map_size + safety * mmap_size.entry_size];

    let _ = system_table.exit_boot_services(handle, &mut mmap).unwrap();

    info!("Going to infinite loop now.");
    inf_loop_0xbeef();
}
