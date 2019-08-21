use super::*;

// Enable the higher half address. Specific for ARM.
fn enable_higher_half() {
unsafe {
    let p4_addr = get_current_p4().start_address().value() as u64;
    let recur_addr:*mut u64 = (p4_addr + RECURSIVE_P4_INDEX as u64 * 8) as *mut u64;
    let flags = EntryFlags::PRESENT | EntryFlags::PAGE | EntryFlags::INNER_SHARE | EntryFlags::ACCESSEDARM;

    *recur_addr = p4_addr | flags.bits();
    let level = 4;
    add_af_flag(p4_addr, level);
    asm!("
        dsb ish;
        isb;" : : : : "volatile");
    asm!("
        ldr x0, = 0x004404FF;
        msr mair_el1, x0;
        ldr x0, =0x00000005B5103510;
        msr tcr_el1, x0;
        isb;
        mrs x0, ttbr0_el1;
        msr ttbr1_el1, x0;
        dsb ish; 
        isb; 
        ldr x0, =0x0000000030d00801;
        msr sctlr_el1, x0;
        isb;" : : : : "volatile");
    };
    tlb::flush_all();
    debug!("Enable higher half page table");          
}

#[cfg(any(target_arch = "aarch64"))]
fn enable_temporary_page(allocator_mutex: &MutexIrqSafe<AreaFrameAllocator>) -> Result<(), &'static str>{
    unsafe {        
        let mut allocator = allocator_mutex.lock();

        let mut alloc_frame = || allocator.allocate_frame().ok_or("couldn't allocate frame");     

        let p3 = try!(alloc_frame());
        let p2 = try!(alloc_frame());
        let p1 = try!(alloc_frame());
        let p4 = get_current_p4();
        let flags = EntryFlags::PRESENT | EntryFlags::PAGE | EntryFlags::INNER_SHARE | EntryFlags::ACCESSEDARM;
        * ((p4.start_address().value() as u64 + KERNEL_TEXT_P4_INDEX as u64 * 8) as *mut u64) = p3.start_address().value() as u64 | flags.bits();
        * ((p3.start_address().value() as u64 + KERNEL_TEXT_P4_INDEX as u64 * 8) as *mut u64) =  p2.start_address().value() as u64 | flags.bits();
        * ((p2.start_address().value() as u64 + KERNEL_TEXT_P4_INDEX as u64 * 8) as *mut u64) =  p1.start_address().value() as u64 | flags.bits();
        asm!("
            dsb ish;
            isb;" : : : : "volatile");

        Ok(())
    }

}



#[cfg(any(target_arch = "aarch64"))]
// Set P1/P2/P3/P4 pages mapped by UEFI as accessible
fn add_af_flag(p4_entry:u64, level:usize) {
    const ADDRESS_MASK:u64 = 0xfffffffffffff000;
    let p4_addr = p4_entry & ADDRESS_MASK;
    unsafe {
        for i in 0..super::ENTRIES_PER_PAGE_TABLE as u64 {
            let addr = (p4_addr + i * 8) as *mut u64;
            if *addr != 0 && (*addr & 0x400 == 0) {
                *addr = *addr | 0x400;
                if level > 2 {
                    add_af_flag(*addr, level - 1);
                }
            }
        }
    }
}


// Set the recursive entry is P4 page
#[cfg(any(target_arch = "aarch64"))]
fn set_recursive(p4_addr:u64) {
    unsafe { 
        let recur_addr:*mut u64 = (p4_addr + RECURSIVE_P4_INDEX as u64 * 8) as *mut u64;
        *recur_addr = p4_addr + 0x0703;
        asm!("
            dsb ish;
            isb;" : : : : "volatile");
    }
}


/// Initializes a new page table and sets up all necessary mappings for the kernel to continue running. 
/// Returns the following tuple, if successful:
/// 
///  * The kernel's new PageTable, which is now currently active,
///  * the kernel's list of VirtualMemoryAreas,
///  * the kernels' text section MappedPages,
///  * the kernels' rodata section MappedPages,
///  * the kernels' data section MappedPages,
///  * the kernel's list of *other* higher-half MappedPages, which should be kept forever,
///  * the kernel's list of identity-mapped MappedPages, which should be dropped before starting the first userspace program. 
///
/// Otherwise, it returns a str error message. 
/// 
/// Note: this was previously called remap_the_kernel.
pub fn init(bt:&BootServices, allocator_mutex: &MutexIrqSafe<AreaFrameAllocator>) 
   -> Result<(PageTable, Vec<VirtualMemoryArea>, MappedPages, MappedPages, MappedPages, Vec<MappedPages>, Vec<MappedPages>), &'static str> {

    //init higher half
    enable_higher_half();
    let p4_frame = get_current_p4();
    set_recursive(p4_frame.start_address().value() as u64);    
    
    let mut page_table = PageTable::from_current();

    // frame is a single frame, and temp_frames1/2 are tuples of 3 Frames each.
    let (new_frame, temp_frames1, temp_frames2) = {
        let mut allocator = allocator_mutex.lock();
        // a quick closure to allocate one frame
        let mut alloc_frame = || allocator.allocate_frame().ok_or("couldn't allocate frame");     
        (
            try!(alloc_frame()),
            (try!(alloc_frame()), try!(alloc_frame()), try!(alloc_frame())),
            (try!(alloc_frame()), try!(alloc_frame()), try!(alloc_frame()))
        )
    };

    //try!(enable_temporary_page(allocator_mutex));

    let mut new_table = PageTable::new_table(&mut page_table, new_frame, TemporaryPage::new(temp_frames1))?;
    
    let mut vmas: [VirtualMemoryArea; 32] = Default::default();
    let mut text_mapped_pages: Option<MappedPages> = None;
    let mut rodata_mapped_pages: Option<MappedPages> = None;
    let mut data_mapped_pages: Option<MappedPages> = None;
    let mut higher_half_mapped_pages: [Option<MappedPages>; 32] = Default::default();
    let mut identity_mapped_pages: [Option<MappedPages>; 32] = Default::default();

    // consumes and auto unmaps temporary page
    try!( page_table.with(&mut new_table, TemporaryPage::new(temp_frames2), |mapper| {        
        // clear out the initially-mapped kernel entries of P4, since we're recreating kernel page tables from scratch.
        // (they were initialized in InactivePageTable::new())
        //let p4 = mapper.p4_mut();

        mapper.p4_mut().clear_entry(KERNEL_TEXT_P4_INDEX);
        mapper.p4_mut().clear_entry(KERNEL_HEAP_P4_INDEX);
        mapper.p4_mut().clear_entry(KERNEL_STACK_P4_INDEX);

        let mut text_start:   Option<(VirtualAddress, PhysicalAddress)> = None;
        let mut text_end:     Option<(VirtualAddress, PhysicalAddress)> = None;
        let mut rodata_start: Option<(VirtualAddress, PhysicalAddress)> = None;
        let mut rodata_end:   Option<(VirtualAddress, PhysicalAddress)> = None;
        let mut data_start:   Option<(VirtualAddress, PhysicalAddress)> = None;
        let mut data_end:     Option<(VirtualAddress, PhysicalAddress)> = None;

        // let text_flags:       Option<EntryFlags> = None;
        // let rodata_flags:     Option<EntryFlags> = None;
        // let data_flags:       Option<EntryFlags> = None;

        // scoped to release the frame allocator lock
        let mut allocator = allocator_mutex.lock(); 

        const EXTRA_MEMORY_INFO_BUFFER_SIZE:usize = 8;
        let mapped_info_size = bt.memory_map_size() + EXTRA_MEMORY_INFO_BUFFER_SIZE * mem::size_of::<MemoryDescriptor>();
        
        let mut buffer = Vec::with_capacity(mapped_info_size);
        unsafe {
            buffer.set_len(mapped_info_size);
        }

        let (_key, mut maps_iter) = bt
            .memory_map(&mut buffer)
            .expect_success("Failed to retrieve UEFI memory map");
        
        let mut kernel_phys_start: PhysicalAddress = PhysicalAddress::new(0)?;

        const DEFAULT:usize = 0;
        const IMAGE_START:usize = 1;
        const UEFI_START:usize = 2;
        let mut address_section = DEFAULT;

        let mut index = 0;

        debug!("Start to map occupied memories in the new page table");
        // Before map the higher half, map these pages to indentical address because UEFI service might be in use after switching.
        loop {
            match maps_iter.next() {
                Some(mapped_pages) => {
                    let start_phys_addr = mapped_pages.phys_start as usize;
                    let size = mapped_pages.page_count as usize * PAGE_SIZE;

                    if kernel_phys_start.value() == 0 {
                        kernel_phys_start = PhysicalAddress::new(start_phys_addr)?;
                    }
                
                    let end_phys_addr;
                    let end_virt_addr;
                    let start_virt_addr;
                    if start_phys_addr < MAX_VIRTUAL_ADDRESS - KERNEL_OFFSET as usize {
                        start_virt_addr = start_phys_addr as usize + KERNEL_OFFSET;
                        end_virt_addr = VirtualAddress::new(start_virt_addr as usize + size)?;
                        end_phys_addr = PhysicalAddress::new(start_phys_addr as usize + size)?;
                    } else {
                        start_virt_addr = start_phys_addr as usize;
                        end_virt_addr = VirtualAddress::new(start_virt_addr as usize + size)?;
                        end_phys_addr = PhysicalAddress::new(start_phys_addr as usize + size)?;
                    }

                    let start_virt_addr = VirtualAddress::new(start_virt_addr as usize)?;         
                    let start_phys_addr = PhysicalAddress::new(start_phys_addr as usize)?;         
                    match mapped_pages.ty {
                         MemoryType::LOADER_DATA => {

                            if address_section == IMAGE_START {
                                data_start = Some((start_virt_addr, start_phys_addr));
                                data_end = Some((end_virt_addr, end_phys_addr));

                                identity_mapped_pages[index] = Some(try!( mapper.map_frames(
                                    FrameRange::from_phys_addr(start_phys_addr, size as usize), 
                                    Page::containing_address(start_virt_addr - KERNEL_OFFSET), 
                                        EntryFlags::NO_EXECUTE | EntryFlags::PAGE, allocator.deref_mut())
                                ));
                                
                                vmas[index] = VirtualMemoryArea::new(start_virt_addr, size as usize, EntryFlags::GLOBAL, ".data");
                                index += 1;

                            }
                        },
                        MemoryType::LOADER_CODE => {
                            if address_section == IMAGE_START {
                                text_start = Some((start_virt_addr, start_phys_addr));
                                text_end = Some((end_virt_addr, end_phys_addr));
                                address_section = UEFI_START;
                                // This partion is not mapped as read-only because in the original mapping by UEFI, it is writable. 
                                // If map this partion as read-only, some UEFI services such as log does not work.
                                // Map is as read-only if UEFI services are of no use after memory::init()
                                identity_mapped_pages[index] = Some(try!( mapper.map_frames(
                                    FrameRange::from_phys_addr(start_phys_addr, size as usize), 
                                    Page::containing_address(start_virt_addr - KERNEL_OFFSET), EntryFlags::PAGE, allocator.deref_mut())
                                ));
                                vmas[index] = VirtualMemoryArea::new(start_virt_addr, size as usize, EntryFlags::GLOBAL, ".data");
                                index += 1;
                            }
                        }
                        _ => {
                            if address_section == UEFI_START {
                                if  rodata_start.is_none() { 
                                    rodata_start = Some((start_virt_addr, start_phys_addr));
                                }
                                match rodata_end {
                                    Some((_current_va, current_pa)) => {
                                        if current_pa < end_phys_addr {
                                            rodata_end = Some((end_virt_addr, end_phys_addr));
                                        } else {
                                            //MMIO is mapped together with other hardware resources later
                                        }
                                    },
                                    None => {
                                        rodata_end = Some((end_virt_addr, end_phys_addr));
                                    }
                                }
                            } else {
                                let start_virt_addr = VirtualAddress::new_canonical(start_phys_addr.value());
                                identity_mapped_pages[index] = Some(try!( mapper.map_frames(
                                    FrameRange::from_phys_addr(start_phys_addr, size as usize), 
                                    Page::containing_address(start_virt_addr), 
                                    EntryFlags::GLOBAL | EntryFlags::PAGE, allocator.deref_mut())
                                ));
                                vmas[index] = VirtualMemoryArea::new(start_virt_addr, size as usize, EntryFlags::GLOBAL, ".conventional");
                                index += 1;
                            }
                        }
                    }

                    if address_section != DEFAULT {
                    } else {
                        address_section = IMAGE_START;
                    }

                },
                None => break,
            }
            //mapped_pages_index += 1;
        }

        // UEFI memory layout
        //conventional
        //image data
        //image code
        //uefi
        //......
        //uefi
        //mmio
        //mmio

        let (text_start_virt,    text_start_phys)    = try!(text_start  .ok_or("Couldn't find start of .text section"));
        let (_text_end_virt,     text_end_phys)      = try!(text_end    .ok_or("Couldn't find end of .text section"));
        let (rodata_start_virt,  rodata_start_phys)  = try!(rodata_start.ok_or("Couldn't find start of .rodata section"));
        let (_rodata_end_virt,   rodata_end_phys)    = try!(rodata_end  .ok_or("Couldn't find end of .rodata section"));
        let (data_start_virt,    data_start_phys)    = try!(data_start  .ok_or("Couldn't find start of .data section"));
        let (_data_end_virt,     data_end_phys)      = try!(data_end    .ok_or("Couldn't find start of .data section"));

        identity_mapped_pages[index] = Some(try!( mapper.map_frames(
            FrameRange::from_phys_addr(rodata_start_phys,  (rodata_end_phys.value() - rodata_start_phys.value()) as usize), 
                Page::containing_address(rodata_start_virt - KERNEL_OFFSET), 
                EntryFlags::PAGE, allocator.deref_mut())
        ));
        vmas[index] = VirtualMemoryArea::new(rodata_start_virt, (rodata_start_phys.value() - rodata_end_phys.value()) as usize,
            EntryFlags::GLOBAL, ".uefi");
        index += 1;

        use super::HARDWARE_START;
        use super::HARDWARE_END;
        let hardware_virt = VirtualAddress::new_canonical(HARDWARE_START as usize);
        // Map hardware to identity for UEFI services
        identity_mapped_pages[index] = Some(try!( mapper.map_frames(
            FrameRange::from_phys_addr(PhysicalAddress::new(HARDWARE_START as usize)?,  (HARDWARE_END - HARDWARE_START) as usize), 
                Page::containing_address(hardware_virt), 
                EntryFlags::PAGE | EntryFlags::ACCESSEDARM | EntryFlags::INNER_SHARE, allocator.deref_mut())
        ));
  
        // Map hardware to higher half for future use
        let hardware_virt = VirtualAddress::new_canonical(HARDWARE_START as usize + KERNEL_OFFSET);
        higher_half_mapped_pages[index] = Some(try!(mapper.map_frames(
            FrameRange::from_phys_addr(PhysicalAddress::new(HARDWARE_START as usize)?,  (HARDWARE_END - HARDWARE_START) as usize), 
                Page::containing_address(hardware_virt), 
                EntryFlags::PAGE | EntryFlags::ACCESSEDARM | EntryFlags::INNER_SHARE, allocator.deref_mut())
        ));
        vmas[index] = VirtualMemoryArea::new(hardware_virt, (HARDWARE_END - HARDWARE_START) as usize,
            EntryFlags::PAGE, ".mmio");
        index += 1;
        
        // now we map the 5 main sections
        text_mapped_pages = Some( try!( mapper.map_frames(
            FrameRange::from_phys_addr(text_start_phys, text_end_phys.value() - text_start_phys.value()), 
            Page::containing_address(text_start_virt), 
            EntryFlags::PAGE | EntryFlags::ACCESSEDARM | EntryFlags::INNER_SHARE | EntryFlags::READONLY, allocator.deref_mut())
        ));
        rodata_mapped_pages = Some( try!( mapper.map_frames(
            FrameRange::from_phys_addr(rodata_start_phys, rodata_end_phys.value() - rodata_start_phys.value()), 
            Page::containing_address(rodata_start_virt), 
            EntryFlags::PAGE | EntryFlags::ACCESSEDARM | EntryFlags::INNER_SHARE |EntryFlags::READONLY, allocator.deref_mut())
        ));
        data_mapped_pages = Some( try!( mapper.map_frames(
            FrameRange::from_phys_addr(data_start_phys, data_end_phys.value() - data_start_phys.value()),
            Page::containing_address(data_start_virt), 
            EntryFlags::PAGE | EntryFlags::ACCESSEDARM | EntryFlags::INNER_SHARE, allocator.deref_mut())
        ));   

        Ok(()) // mapping closure completed successfully

    })); // TemporaryPage is dropped here

    let text_mapped_pages   = try!(text_mapped_pages  .ok_or("Couldn't map .text section"));
    let rodata_mapped_pages = try!(rodata_mapped_pages.ok_or("Couldn't map .rodata section"));
    let data_mapped_pages   = try!(data_mapped_pages  .ok_or("Couldn't map .data section"));

    debug!("switching to new page table {:?}", new_table);
    let mut new_page_table = page_table.switch(&new_table); 
    // here, new_page_table and new_table should be identical
    debug!("switched to new page table {:?}.", new_page_table); 

    // After this point, we must "forget" all of the above mapped_pages instances if an error occurs,
    // because they will be auto-unmapped from the new page table upon return, causing all execution to stop.          

    // We must map the heap memory here, before it can initialized! 
    let (heap_mapped_pages, heap_vma) = {
        let mut allocator = allocator_mutex.lock();

        let pages = PageRange::from_virt_addr(VirtualAddress::new_canonical(KERNEL_HEAP_START), KERNEL_HEAP_INITIAL_SIZE);
        let heap_flags = paging::EntryFlags::WRITABLE;
        let heap_vma: VirtualMemoryArea = VirtualMemoryArea::new(VirtualAddress::new_canonical(KERNEL_HEAP_START), KERNEL_HEAP_INITIAL_SIZE, heap_flags, "Kernel Heap");
        let heap_mp = try_forget!(
            new_page_table.map_pages(pages, heap_flags, allocator.deref_mut())
                .map_err(|e| {
                    error!("Failed to map kernel heap memory pages, {} bytes starting at virtual address {:#X}. Error: {:?}", KERNEL_HEAP_INITIAL_SIZE, KERNEL_HEAP_START, e);
                    "Failed to map the kernel heap memory. Perhaps the KERNEL_HEAP_INITIAL_SIZE exceeds the size of the system's physical memory?"
                }),
            text_mapped_pages, rodata_mapped_pages, data_mapped_pages, higher_half_mapped_pages, identity_mapped_pages
        );
        heap_irq_safe::init(KERNEL_HEAP_START, KERNEL_HEAP_INITIAL_SIZE);
        
        allocator.alloc_ready(); // heap is ready
        (heap_mp, heap_vma)
    };

    // debug!("mapped and inited the heap, VMA: {:?}", heap_vma);
    // HERE: now the heap is set up, we can use dynamically-allocated types like Vecs

    let mut kernel_vmas: Vec<VirtualMemoryArea> = vmas.to_vec();
    kernel_vmas.retain(|x|  *x != VirtualMemoryArea::default() );
    kernel_vmas.push(heap_vma);

    debug!("kernel_vmas: {:?}", kernel_vmas);

    let mut higher_half: Vec<MappedPages> = higher_half_mapped_pages.iter_mut().filter_map(|opt| opt.take()).collect();
    higher_half.push(heap_mapped_pages);
    let identity: Vec<MappedPages> = identity_mapped_pages.iter_mut().filter_map(|opt| opt.take()).collect();

    Ok((new_page_table, kernel_vmas, text_mapped_pages, rodata_mapped_pages, data_mapped_pages, higher_half, identity))
}
