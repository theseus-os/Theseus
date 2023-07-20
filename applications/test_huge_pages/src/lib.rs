#![no_std]
#[macro_use] extern crate alloc;
#[macro_use] extern crate app_io;

extern crate memory;

use alloc::vec::Vec;
use alloc::string::String;

use memory::{
    Page, PteFlags, PteFlagsArch,
    VirtualAddress, PhysicalAddress, 
    Page1GiB, Page2MiB, 
    PAGE_2MB_SIZE, PAGE_1GB_SIZE, 
    allocate_2mb_pages, allocate_1gb_pages, allocate_frames_at
};

pub fn main(_args: Vec<String>) -> isize {
    let kernel_mmi_ref = memory::get_kernel_mmi_ref().ok_or("KERNEL_MMI was not yet initialized!").unwrap();            

    let page1g = Page::<Page1GiB>::containing_address_1GiB(VirtualAddress::zero());
    println!("Size of page 1 is {:?}", page1g.page_size());
    let page2m = Page::<Page2MiB>::containing_address_2MiB(VirtualAddress::zero());
    println!("Size of page 2 is {:?}", page2m.page_size());
    
    
    match page1g.page_size() {
        PAGE_1GB_SIZE => println!("Page 1 recognized as 1GiB"),
        _ => println!("Page 1 not recognized as 1GiB"),
    }
    match page2m.page_size() {
        PAGE_2MB_SIZE => println!("Page 2 recognized as 2MiB"),
        _ => println!("Page 2 not recognized as 2MiB"),
    }

    let allocated_1g = allocate_1gb_pages(1)
        .ok_or("test_huge_pages: could not allocate 1GiB page")
        .unwrap();
    let allocated_2m = allocate_2mb_pages(1)
        .ok_or("test_huge_pages: could not allocate 1GiB page")
        .unwrap();
    println!("Huge pages successfully allocated!");


    // kernel_mmi_ref.lock().page_table.map_allocated_pages(
    //     allocated_1g,
    //     PteFlags::new().valid(true).writable(true),
    // ).expect("test_huge_pages: call to map_allocated_pages failed");
    // let allocated_2m_frames = allocate_frames_at(
    //     PhysicalAddress::new(3 * 512 * 512).unwrap(),
    //     512).unwrap();


    // kernel_mmi_ref.lock().page_table.map_allocated_pages_to(
    //     allocated_2m,
    //     allocated_2m_frames,
    //     PteFlags::new().valid(true).writable(true),
    // ).expect("test_huge_pages: call to map_allocated_pages failed");

    kernel_mmi_ref.lock().page_table.map_allocated_pages(
        allocated_2m,
        PteFlags::new().valid(true).writable(true),
    ).expect("test_huge_pages: call to map_allocated_pages failed");

    0
}

