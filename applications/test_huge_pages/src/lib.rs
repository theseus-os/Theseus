#![no_std]
#[macro_use] extern crate alloc;
#[macro_use] extern crate app_io;

extern crate memory;
extern crate frame_allocator;
extern crate page_allocator;

use alloc::vec::Vec;
use alloc::string::String;
use memory::{
    Page, PteFlags,
    VirtualAddress, PhysicalAddress, 
    Page1G, Page2M,
    allocate_frames_at, allocate_pages_at
};
// For checking valid test addresses, if necessary
// use frame_allocator::dump_frame_allocator_state;
// use page_allocator::dump_page_allocator_state;

pub fn main(_args: Vec<String>) -> isize {
    println!("NOTE: Before running, make sure you Theseus has been run with enough memory (make orun QEMU_MEMORY=3G).");
    
    let kernel_mmi_ref = memory::get_kernel_mmi_ref()
        .ok_or("KERNEL_MMI was not yet initialized!")
        .unwrap();
    
    // Preliminary tests
    let page1g = Page::<Page1G>::containing_address_1gb(VirtualAddress::zero());
    println!("Size of page 1 is {:?}", page1g.page_size());
    let page2m = Page::<Page2M>::containing_address_2mb(VirtualAddress::zero());
    println!("Size of page 2 is {:?}", page2m.page_size());
    
    
    // match page1g.page_size() {
    //     PAGE_1GB_SIZE => println!("Page 1 recognized as 1GiB"),
    //     _ => println!("Page 1 not recognized as 1GiB"),
    // }
    // match page2m.page_size() {
    //     PAGE_2MB_SIZE => println!("Page 2 recognized as 2MiB"),
    //     _ => println!("Page 2 not recognized as 2MiB"),
    // }
    // let _allocated_1g = allocate_1gb_pages(1)
    //     .ok_or("test_huge_pages: could not allocate 1GiB page")
    //     .unwrap();
    // let _allocated_2m = allocate_2mb_pages(1)
    //     .ok_or("test_huge_pages: could not allocate 2MiB page")
    //     .unwrap();
    // println!("Huge pages successfully allocated!");
    // // end preliminary tests

    let mut aligned_2mb_page = allocate_pages_at(
        VirtualAddress::new_canonical(0x60000000),
        512).expect("Could not allocate pages. Make sure you have enough memory for the kernel (compile with make orun QEMU_MEMORY=3G).");
    aligned_2mb_page.to_2mb_allocated_pages();

    // frame allocator has not been modified to deal with huge frames yet
    let aligned_4k_frames = allocate_frames_at(
            PhysicalAddress::new_canonical(0x60000000),
            512).expect("Could not allocate frames. Make sure you have enough memory for the kernel (compile with make orun QEMU_MEMORY=3G).");

    let allocated_2mb_frames = aligned_4k_frames.into_2mb_allocated_frames().expect("Could not convert range of allocated frames into huge allocated frames");

    let mut mapped_2mb_page = kernel_mmi_ref.lock().page_table.map_allocated_pages_to(
        aligned_2mb_page,
        allocated_2mb_frames,
        PteFlags::new().valid(true).writable(true),
    ).expect("test_huge_pages: call to map_allocated_pages failed");
    
    kernel_mmi_ref.lock().page_table.dump_pte(mapped_2mb_page.start_address());

    // See if address can be translated
    let translated = kernel_mmi_ref.lock()
        .page_table.translate(mapped_2mb_page.start_address())
        .unwrap();
    println!("Virtual address {} -> {}", mapped_2mb_page.start_address(), translated);
    
    
    // Testing mem access with huge pages
    let val = mapped_2mb_page
        .as_type_mut::<u8>(0)
        .expect("test huge pages: call to as_type_mut() on mapped 2Mb page failed");
    println!("Value at offset of 2Mib huge page before updating is {}", *val);

    *val = 35;
    
    let updated_val = mapped_2mb_page
        .as_type::<u8>(0)
        .expect("test huge pages: call to as_type() on mapped 2Mb page failed");
    println!("Value at offset of 2Mib huge page after updating is {}", *updated_val);
    
    // Dump entries to see if dropping has unmapped everything properly
    drop(mapped_2mb_page);
    kernel_mmi_ref.lock().page_table.dump_pte(VirtualAddress::new_canonical(0x20000000));
    0
}
