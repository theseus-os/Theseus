#![no_std]
#[macro_use] extern crate alloc;
#[macro_use] extern crate app_io;

extern crate memory;

use alloc::vec::Vec;
use alloc::string::String;
use memory::{
    PteFlags,
    VirtualAddress, PhysicalAddress, 
    allocate_frames_at, allocate_pages_at
};

pub fn main(_args: Vec<String>) -> isize {
    println!("NOTE: Before running, make sure you Theseus has been run with enough memory (make orun QEMU_MEMORY=5G).");
    let kernel_mmi_ref = memory::get_kernel_mmi_ref()
        .ok_or("KERNEL_MMI was not yet initialized!")
        .unwrap();
    
    // Now the same for 1gb pages
    let mut aligned_1gb_page = allocate_pages_at(
        VirtualAddress::new_canonical(0x40000000), 
        512*512).expect("Failed to allocate range for 1GiB page. Make sure you have enough memory for the kernel (compile with make orun QEMU_MEMORY=3G).");
    aligned_1gb_page.to_1gb_allocated_pages();

    let aligned_4k_frames = allocate_frames_at(
        PhysicalAddress::new_canonical(0x40000000), //0x1081A000
        512 * 512).expect("Failed to allocate enough frames at desired address. Make sure you have enough memory for the kernel (compile with make orun QEMU_MEMORY=3G).");

    let mut mapped_1gb_page = kernel_mmi_ref.lock().page_table.map_allocated_pages_to(
        aligned_1gb_page,
        aligned_4k_frames,
        PteFlags::new().valid(true).writable(true),
    ).expect("test_huge_pages: call to map_allocated_pages failed for 1GiB page");
        
    kernel_mmi_ref.lock().page_table.dump_pte(mapped_1gb_page.start_address());
    
    // See if address can be translated
    let translated = kernel_mmi_ref.lock()
        .page_table.translate(VirtualAddress::new_canonical(0x40000000))
        .unwrap();
    println!("Virtual address 0x40000000-> {}", translated);

    let val = mapped_1gb_page
        .as_type_mut::<u8>(0)
        .expect("test huge pages: call to as_type_mut() on mapped 2Mb page failed");
    println!("Value at offset of 1GiB huge page before updating is {}", *val);

    *val = 35;

    let updated_val = mapped_1gb_page
        .as_type::<u8>(0)
        .expect("test huge pages: call to as_type() on mapped 2Mb page failed");
    println!("Value at offset of 1GiB huge page after updating is {}", *updated_val);

    // Dump entries to see if dropping has unmapped everything properly
    drop(mapped_1gb_page);
    kernel_mmi_ref.lock().page_table.dump_pte(VirtualAddress::new_canonical(0x40000000));   
    
    0
}
