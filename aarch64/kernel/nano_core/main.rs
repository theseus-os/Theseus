#![feature(naked_functions)]
#![feature(abi_efiapi)]
#![no_std]
#![no_main]

extern crate alloc;
extern crate logger;
extern crate frame_allocator;
extern crate page_allocator;
extern crate memory_structs;
extern crate kernel_config;

use alloc::vec;
use core::arch::asm;

use uefi::{prelude::entry, Status, Handle, table::{SystemTable, Boot}};

use frame_allocator::{PhysicalMemoryRegion, MemoryRegionType};
use memory_structs::{VirtualAddress, PhysicalAddress, Frame, FrameRange};
use kernel_config::memory::PAGE_SIZE;

use log::{info, error};

mod uefi_conv;

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

    // Identifying free and reserved regions so we can initialize the frame allocator.
    let mut free_regions: [Option<PhysicalMemoryRegion>; 32] = Default::default();
    let mut free_index = 0;
    let mut reserved_regions: [Option<PhysicalMemoryRegion>; 32] = Default::default();
    let mut reserved_index = 0;

    for descriptor in mem_iter {
        let page_count = descriptor.page_count as usize;
        let size = page_count * PAGE_SIZE;
        if size > 0 {
            let region_type = uefi_conv::convert_mem(descriptor.ty);
            let (dst, index) = match region_type {
                MemoryRegionType::Free => (&mut free_regions, &mut free_index),
                MemoryRegionType::Reserved => (&mut reserved_regions, &mut reserved_index),
                MemoryRegionType::Unknown => continue,
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

    frame_allocator::init(free_regions.iter().flatten(), reserved_regions.iter().flatten())?;
    info!("Initialized new frame allocator!");
    frame_allocator::dump_frame_allocator_state();

    // On x86_64 `page_allocator` is initialized with a value obtained
    // from the ELF layout. Here I'm choosing a value which is probably
    // valid (uneducated guess); once we have an ELF aarch64 kernel
    // we'll be able to use the original limit defined with KERNEL_OFFSET
    // and the ELF layout.
    page_allocator::init(VirtualAddress::new_canonical(0x100_000_000))?;
    info!("Initialized new page allocator!");
    page_allocator::dump_page_allocator_state();

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
