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
    allocate_2mb_pages, allocate_1gb_pages, allocate_frames_at, allocate_pages_at
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

    let _allocated_1g = allocate_1gb_pages(1)
        .ok_or("test_huge_pages: could not allocate 1GiB page")
        .unwrap();
    let _allocated_2m = allocate_2mb_pages(1)
        .ok_or("test_huge_pages: could not allocate 1GiB page")
        .unwrap();
    println!("Huge pages successfully allocated!");

    /*
    Examples of addresses that work
    let aligned_2m_page = allocate_pages_at(VirtualAddress::new_canonical(0x20000000), 512)
        .unwrap()
        .as_allocated_2mb();
    let aligned_2m_page = allocate_pages_at(VirtualAddress::new_canonical(0x100000000), 512)
        .unwrap()
        .as_allocated_2mb();
     */

    let aligned_2m_page = allocate_pages_at(VirtualAddress::new_canonical(0x1000000), 512)
        .unwrap()
        .as_allocated_2mb();

    // frame allocator has not been modified to deal with huge frames
    let aligned_4k_frames = allocate_frames_at(PhysicalAddress::new_canonical(0x20000000), 512).unwrap();

    let mapped_pages = kernel_mmi_ref.lock().page_table.map_allocated_pages_to(
        aligned_2m_page,
        aligned_4k_frames,
        PteFlags::new().valid(true).writable(true),
    ).expect("test_huge_pages: call to map_allocated_pages failed");



    0
}