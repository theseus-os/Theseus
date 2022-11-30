#![feature(naked_functions)]
#![feature(abi_efiapi)]
#![no_std]
#![no_main]

extern crate alloc;
extern crate logger;
extern crate frame_allocator;
extern crate page_allocator;
extern crate memory_structs;
extern crate memory;

use alloc::vec;
use core::arch::asm;
use alloc::vec::Vec;

use uefi::{prelude::entry, Status, Handle, table::{SystemTable, Boot, boot::MemoryType}};

use frame_allocator::{PhysicalMemoryRegion, MemoryRegionType};
use memory_structs::{PAGE_SIZE, PteFlags, PhysicalAddress, Frame, FrameRange};

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

    let dummy = [ 0xdeadbeefu32 ];

    info!("dummy address: {:?}", dummy.as_ptr());

    let mmap_size = boot_svc.memory_map_size();
    let mut mmap = vec![0; mmap_size.map_size + safety * mmap_size.entry_size];

    let mut mapped_regions = 0;
    {
        let (_, layout) = boot_svc.memory_map(&mut mmap).unwrap();
        for descriptor in layout {
            if descriptor.ty != MemoryType::CONVENTIONAL {
                mapped_regions += 1;
            }
        }
    }
    let mut mapped_regions = Vec::with_capacity(mapped_regions + safety);

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
                MemoryRegionType::Unknown => (&mut reserved_regions, &mut reserved_index),
            };

            let start_addr = descriptor.phys_start as usize;
            let start_addr = PhysicalAddress::new_canonical(start_addr);
            let end_addr = start_addr + size;

            let first_frame = Frame::containing_address(start_addr);
            let last_frame = Frame::containing_address(end_addr - 1);

            let range = FrameRange::new(first_frame, last_frame);

            if let Some(flags) = uefi_conv::get_mem_flags(descriptor.ty) {
                // info!("{:?} ({}) -> {:?} -> {:?}", start_addr, page_count, flags, descriptor.ty);
                mapped_regions.push((start_addr, page_count, flags));
            }

            if region_type != MemoryRegionType::Unknown {
                let region = PhysicalMemoryRegion::new(range, region_type);
                dst[*index] = Some(region);
                *index += 1;
            }
        }
    }

    let uart_phys_addr = PhysicalAddress::new(0x0900_0000).unwrap();
    mapped_regions.push((uart_phys_addr, 1, PteFlags::DEVICE_MEMORY | PteFlags::NOT_EXECUTABLE | PteFlags::WRITABLE));

    info!("Calling memory::init();");
    let iter = mapped_regions.drain(..);
    info!("page table: {:?}", memory::init(&free_regions, &reserved_regions, iter));

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
