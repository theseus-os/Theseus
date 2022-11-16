#![feature(naked_functions)]
#![feature(abi_efiapi)]
#![no_std]
#![no_main]

extern crate alloc;
extern crate logger;
extern crate frame_allocator;
extern crate page_allocator;
extern crate memory_structs;

use alloc::vec;
use core::arch::asm;

use uefi::{prelude::entry, Status, Handle, table::{SystemTable, Boot, boot::MemoryType}};

use frame_allocator::{PhysicalMemoryRegion, MemoryRegionType};
use memory_structs::{PAGE_SIZE, PhysicalAddress, Frame, FrameRange, VirtualAddress};

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

    let boot_svc = system_table.boot_services();

    let safety = 16;

    let mmap_size = boot_svc.memory_map_size();
    let mut mmap = vec![0; mmap_size.map_size + safety * mmap_size.entry_size];

    let (_runtime_svc, mem_iter) = system_table.exit_boot_services(handle, &mut mmap)
        .map_err(|_| "nano_core::main - couldn't exit uefi boot services")?;

    // Now set up the list of free regions and reserved regions so we can initialize the frame allocator.
    let mut free_regions: [Option<PhysicalMemoryRegion>; 32] = Default::default();
    let mut free_index = 0;
    let mut reserved_regions: [Option<PhysicalMemoryRegion>; 32] = Default::default();
    let mut reserved_index = 0;

    for descriptor in mem_iter {
        let size = descriptor.page_count as usize * PAGE_SIZE;
        if size > 0 {
            let (dst, index, region_type) = match descriptor.ty {
                MemoryType::CONVENTIONAL => (&mut free_regions, &mut free_index, MemoryRegionType::Free),
                _ => (&mut reserved_regions, &mut reserved_index, MemoryRegionType::Reserved),
            };

            let start_addr = descriptor.phys_start as usize;
            let start_addr = PhysicalAddress::new_canonical(start_addr);
            let end_addr = start_addr + size;

            let first_frame = Frame::containing_address(start_addr);
            let last_frame = Frame::containing_address(end_addr - 1);

            let range = FrameRange::new(first_frame, last_frame);
            let region = PhysicalMemoryRegion::new(range, region_type);

            dst[*index] = Some(region);
            *index += 1;
        }
    }

    let _callback = frame_allocator::init(
        free_regions.iter().flatten(),
        reserved_regions.iter().flatten(),
    )?;

    // for now, virtual addresses will be above 4GB
    page_allocator::init(VirtualAddress::new_canonical(0x100_000_000))?;

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
