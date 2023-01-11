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
extern crate kernel_config;

use core::arch::asm;
use alloc::vec::Vec;
use alloc::vec;

use uefi::{prelude::entry, Status, Handle, table::{SystemTable, Boot, boot::MemoryType}};

use frame_allocator::MemoryRegionType;
use memory_structs::{PhysicalAddress, FrameRange};
use kernel_config::memory::PAGE_SIZE;
use pte_flags::PteFlags;

use log::{info, error};

mod uefi_conv;
mod context_switch;

use context_switch::{create_stack, switch_to_task, task_entrypoint};

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

    let mut layout_len = 0;
    {
        let (_, layout) = boot_svc.memory_map(&mut mmap).unwrap();
        for descriptor in layout {
            if descriptor.ty != MemoryType::CONVENTIONAL && descriptor.page_count > 0 {
                layout_len += 1;
            }
        }
    }
    let mut layout_vec = Vec::with_capacity(layout_len + safety);

    let (_runtime_svc, mem_iter) = system_table.exit_boot_services(handle, &mut mmap)
        .map_err(|_| "nano_core::main - couldn't exit uefi boot services")?;

    for descriptor in mem_iter {
        let page_count = descriptor.page_count as usize;
        let size = page_count * PAGE_SIZE;
        if size > 0 {
            let mem_type = uefi_conv::convert_mem(descriptor.ty);
            let flags = uefi_conv::get_mem_flags(descriptor.ty);

            let start_addr = descriptor.phys_start as usize;
            let start_addr = PhysicalAddress::new_canonical(start_addr);
            let range = FrameRange::from_phys_addr(start_addr, size);

            layout_vec.push((range, mem_type, flags));
        }
    }

    // I'm also using this utility function for GIC mmio mapping
    let mmio_region = |phys_addr, num_pages| {
        let phys_addr = PhysicalAddress::new(phys_addr).unwrap();
        let range = FrameRange::from_phys_addr(phys_addr, num_pages);
        let flags = PteFlags::DEVICE_MEMORY | PteFlags::NOT_EXECUTABLE | PteFlags::WRITABLE;
        (range, MemoryRegionType::Free, Some(flags))
    };

    layout_vec.push(mmio_region(0x0900_0000, 1));

    info!("Calling memory::init();");
    let mut page_table = memory::init(&layout_vec)?;
    info!("page table: {:?}", page_table);

    info!("Creating new stack");
    let (_new_stack, stack_ptr) = create_stack(&mut page_table, task_entrypoint, 16)?;

    info!("Switching to new task");
    switch_to_task(stack_ptr);

    info!("[in main]");
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
