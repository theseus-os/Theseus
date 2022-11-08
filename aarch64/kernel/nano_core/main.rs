#![feature(naked_functions)]
#![feature(abi_efiapi)]
#![no_std]
#![no_main]

extern crate alloc;
extern crate logger;

use alloc::vec;
use core::arch::asm;

use uefi::{prelude::entry, Status, Handle, table::{SystemTable, Boot}};

use log::{info, error};

#[inline(never)]
extern "C" fn inf_loop_0xbeef() -> ! {
    unsafe { asm!("mov x1, #0xbeef") };
    loop {}
}

fn main(
    handle: Handle,
    mut system_table: SystemTable<Boot>,
) -> Result<(), &'static str> {

    logger::init()?;
    info!("Hello, World!");

    uefi_services::init(&mut system_table)
        .map_err(|_| "nano_core::main - couldn't init uefi services")?;

    let bootsvc = system_table.boot_services();

    let safety = 16;

    let mmap_size = bootsvc.memory_map_size();
    let mut mmap = vec![0; mmap_size.map_size + safety * mmap_size.entry_size];

    let _ = system_table.exit_boot_services(handle, &mut mmap)
        .map_err(|_| "nano_core::main - couldn't exit uefi boot services")?;

    info!("Going to infinite loop now.");
    inf_loop_0xbeef();

}

#[entry]
fn uefi_main(
    handle: Handle,
    system_table: SystemTable<Boot>,
) -> Status {
    match main(handle, system_table) {
        Ok(()) => Status::SUCCESS,
        Err(msg) => {
            error!("{}", msg);
            Status::ABORTED
        },
    }
}
