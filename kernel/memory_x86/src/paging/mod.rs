use super::*;

use super::x86_64::registers::control_regs;


impl PageTableOperator for PageTable {
    /// Switches from the currently-active page table (this `PageTable`, i.e., `self`) to the given `new_table`.
    /// Returns the newly-switched-to PageTable.
    pub fn switch(&mut self, new_table: &PageTable) -> PageTable {
        // perform the actual page table switch
        unsafe {
            control_regs::cr3_write(x86_64::PhysicalAddress(new_table.p4_table.start_address().value() as u64));
        }
}

/// Returns the current top-level page table frame, e.g., cr3 on x86
pub fn get_current_p4() -> Frame {
    Frame::containing_address(PhysicalAddress::new_canonical(control_regs::cr3().0 as usize))
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
pub fn init(allocator_mutex: &MutexIrqSafe<AreaFrameAllocator>, boot_info: &multiboot2::BootInformation) 
    -> Result<(PageTable, Vec<VirtualMemoryArea>, MappedPages, MappedPages, MappedPages, Vec<MappedPages>, Vec<MappedPages>), &'static str>
{
    // bootstrap a PageTable from the currently-loaded page table
    let mut page_table = PageTable::from_current(get_current_p4());

    let boot_info_start_vaddr = VirtualAddress::new(boot_info.start_address())?;
    let boot_info_start_paddr = page_table.translate(boot_info_start_vaddr).ok_or("Couldn't get boot_info start physical address")?;
    let boot_info_end_vaddr = VirtualAddress::new(boot_info.end_address())?;
    let boot_info_end_paddr = page_table.translate(boot_info_end_vaddr).ok_or("Couldn't get boot_info end physical address")?;
    let boot_info_size = boot_info.total_size();
    info!("multiboot start: {:#X}-->{:#X}, multiboot end: {:#X}-->{:#X}, size: {:#X}\n",
            boot_info_start_vaddr, boot_info_start_paddr, boot_info_end_vaddr, boot_info_end_paddr, boot_info_size
    );

    // new_frame is a single frame, and temp_frames1/2 are tuples of 3 Frames each.
    let (new_frame, temp_frames1, temp_frames2) = {
        let mut allocator = allocator_mutex.lock();
        // a quick closure to allocate one frame
        let mut alloc_frame = || allocator.allocate_frame().ok_or("couldn't allocate frame"); 
        (
            alloc_frame()?,
            (alloc_frame()?, alloc_frame()?, alloc_frame()?),
            (alloc_frame()?, alloc_frame()?, alloc_frame()?)
        )
    };
    let mut new_table = PageTable::new_table(&mut page_table, new_frame, TemporaryPage::new(temp_frames1))?;

    let elf_sections_tag = try!(boot_info.elf_sections_tag().ok_or("no Elf sections tag present!"));   
    let mut vmas: [VirtualMemoryArea; 32] = Default::default();
    let mut text_mapped_pages: Option<MappedPages> = None;
    let mut rodata_mapped_pages: Option<MappedPages> = None;
    let mut data_mapped_pages: Option<MappedPages> = None;
    let mut higher_half_mapped_pages: [Option<MappedPages>; 32] = Default::default();
    let mut identity_mapped_pages: [Option<MappedPages>; 32] = Default::default();

    // consumes and auto unmaps temporary page
    try!( page_table.with(&mut new_table, TemporaryPage::new(temp_frames2), |mapper| {
        
        // clear out the initially-mapped kernel entries of P4, since we're recreating kernel page tables from scratch.
        // (they were initialized in PageTable::new_table())
        mapper.p4_mut().clear_entry(KERNEL_TEXT_P4_INDEX);
        mapper.p4_mut().clear_entry(KERNEL_HEAP_P4_INDEX);
        mapper.p4_mut().clear_entry(KERNEL_STACK_P4_INDEX);


        let mut text_start:   Option<(VirtualAddress, PhysicalAddress)> = None;
        let mut text_end:     Option<(VirtualAddress, PhysicalAddress)> = None;
        let mut rodata_start: Option<(VirtualAddress, PhysicalAddress)> = None;
        let mut rodata_end:   Option<(VirtualAddress, PhysicalAddress)> = None;
        let mut data_start:   Option<(VirtualAddress, PhysicalAddress)> = None;
        let mut data_end:     Option<(VirtualAddress, PhysicalAddress)> = None;

        let mut text_flags:       Option<EntryFlags> = None;
        let mut rodata_flags:     Option<EntryFlags> = None;
        let mut data_flags:       Option<EntryFlags> = None;


        // scoped to release the frame allocator lock
        {
            let mut allocator = allocator_mutex.lock(); 

            let mut index = 0;    
            // map the allocated kernel text sections
            for section in elf_sections_tag.sections() {
                
                // skip sections that don't need to be loaded into memory
                if section.size() == 0 
                    || !section.is_allocated() 
                    || section.name().starts_with(".gcc")
                    || section.name().starts_with(".eh_frame")
                    || section.name().starts_with(".debug") 
                {
                    continue;
                }
                
                debug!("Looking at loaded section {} at {:#X}, size {:#X}", section.name(), section.start_address(), section.size());

                if PhysicalAddress::new_canonical(section.start_address() as usize).frame_offset() != 0 {
                    error!("Section {} at {:#X}, size {:#X} was not page-aligned!", section.name(), section.start_address(), section.size());
                    return Err("Kernel ELF Section was not page-aligned");
                }

                let flags = EntryFlags::from_multiboot2_section_flags(&section) | EntryFlags::GLOBAL;

                // even though the linker stipulates that the kernel sections have a higher-half virtual address,
                // they are still loaded at a lower physical address, in which phys_addr = virt_addr - KERNEL_OFFSET.
                // thus, we must map the zeroeth kernel section from its low address to a higher-half address,
                // and we must map all the other sections from their higher given virtual address to the proper lower phys addr
                let mut start_phys_addr = section.start_address() as usize;
                if start_phys_addr >= KERNEL_OFFSET { 
                    // true for all sections but the first section (inittext)
                    start_phys_addr -= KERNEL_OFFSET;
                }
                
                let mut start_virt_addr = section.start_address() as usize;
                if start_virt_addr < KERNEL_OFFSET { 
                    // special case to handle the first section only
                    start_virt_addr += KERNEL_OFFSET;
                }

                let start_phys_addr = PhysicalAddress::new(start_phys_addr)?;
                let start_virt_addr = VirtualAddress::new(start_virt_addr)?;
                let end_virt_addr = start_virt_addr + (section.size() as usize);
                let end_phys_addr = start_phys_addr + (section.size() as usize);


                // the linker script (linker_higher_half.ld) defines the following order of sections:
                //     .init (start) then .text (end)
                //     .data (start) then .bss (end)
                //     .rodata (start and end)
                // Those are the only sections we care about.
                let static_str_name = match section.name() {
                    ".init" => {
                        text_start = Some((start_virt_addr, start_phys_addr));
                        "nano_core .init"
                    } 
                    ".text" => {
                        text_end = Some((end_virt_addr, end_phys_addr));
                        text_flags = Some(flags);
                        "nano_core .text"
                    }
                    ".rodata" => {
                        rodata_start = Some((start_virt_addr, start_phys_addr));
                        rodata_end   = Some((end_virt_addr, end_phys_addr));
                        rodata_flags = Some(flags);
                        "nano_core .rodata"
                    }
                    ".data" => {
                        data_start = Some((start_virt_addr, start_phys_addr));
                        data_flags = Some(flags);
                        "nano_core .data"
                    }
                    ".bss" => {
                        data_end = Some((end_virt_addr, end_phys_addr));
                        "nano_core .bss"
                    }
                    _ =>  {
                        error!("Section {} at {:#X}, size {:#X} was not an expected section (.init, .text, .data, .bss, .rodata)", 
                                section.name(), section.start_address(), section.size());
                        return Err("Kernel ELF Section had an unexpected name (expected .init, .text, .data, .bss, .rodata)");
                    }
                };
                vmas[index] = VirtualMemoryArea::new(start_virt_addr, section.size() as usize, flags, static_str_name);
                debug!("     mapping kernel section: {} at addr: {:?}", section.name(), vmas[index]);


                // to allow the APs to boot up, we identity map the kernel sections too.
                // (lower half virtual addresses mapped to same lower half physical addresses)
                // we will unmap these later before we start booting to userspace processes
                identity_mapped_pages[index] = Some(
                    mapper.map_frames(
                        FrameRange::from_phys_addr(start_phys_addr, section.size() as usize), 
                        Page::containing_address(start_virt_addr - KERNEL_OFFSET), 
                        flags,
                        allocator.deref_mut()
                    )?
                );
                debug!("           also mapped vaddr {:#X} to paddr {:#x} (size {:#X})", start_virt_addr - KERNEL_OFFSET, start_phys_addr, section.size());

                index += 1;      

            } // end of section iterator


            let (text_start_virt,    text_start_phys)    = text_start  .ok_or("Couldn't find start of .text section")?;
            let (_text_end_virt,     text_end_phys)      = text_end    .ok_or("Couldn't find end of .text section")?;
            let (rodata_start_virt,  rodata_start_phys)  = rodata_start.ok_or("Couldn't find start of .rodata section")?;
            let (_rodata_end_virt,   rodata_end_phys)    = rodata_end  .ok_or("Couldn't find end of .rodata section")?;
            let (data_start_virt,    data_start_phys)    = data_start  .ok_or("Couldn't find start of .data section")?;
            let (_data_end_virt,     data_end_phys)      = data_end    .ok_or("Couldn't find start of .data section")?;

            let text_flags    = text_flags  .ok_or("Couldn't find .text section flags")?;
            let rodata_flags  = rodata_flags.ok_or("Couldn't find .rodata section flags")?;
            let data_flags    = data_flags  .ok_or("Couldn't find .data section flags")?;


            // now we map the 5 main sections into 3 groups according to flags
            text_mapped_pages = Some( try!( mapper.map_frames(
                FrameRange::from_phys_addr(text_start_phys, text_end_phys.value() - text_start_phys.value()), 
                Page::containing_address(text_start_virt), 
                text_flags, allocator.deref_mut())
            ));
            rodata_mapped_pages = Some( try!( mapper.map_frames(
                FrameRange::from_phys_addr(rodata_start_phys, rodata_end_phys.value() - rodata_start_phys.value()), 
                Page::containing_address(rodata_start_virt), 
                rodata_flags, allocator.deref_mut())
            ));
            data_mapped_pages = Some( try!( mapper.map_frames(
                FrameRange::from_phys_addr(data_start_phys, data_end_phys.value() - data_start_phys.value()),
                Page::containing_address(data_start_virt), 
                data_flags, allocator.deref_mut())
            ));

            // map the VGA display memory as writable, which technically goes from 0xA_0000 - 0xC_0000 (exclusive),
            // VGA text mode only goes from 0xB_8000 - 0XC_0000
            const VGA_DISPLAY_PHYS_START: usize = 0xA_0000;
            const VGA_DISPLAY_PHYS_END: usize = 0xC_0000;
            const VGA_SIZE_IN_BYTES: usize = VGA_DISPLAY_PHYS_END - VGA_DISPLAY_PHYS_START;
            let vga_display_virt_addr = VirtualAddress::new_canonical(VGA_DISPLAY_PHYS_START + KERNEL_OFFSET);
            let vga_display_flags = EntryFlags::PRESENT | EntryFlags::WRITABLE | EntryFlags::GLOBAL | EntryFlags::NO_CACHE;
            higher_half_mapped_pages[index] = Some( try!( mapper.map_frames(
                FrameRange::from_phys_addr(PhysicalAddress::new(VGA_DISPLAY_PHYS_START)?, VGA_SIZE_IN_BYTES), 
                Page::containing_address(vga_display_virt_addr), 
                vga_display_flags,
                allocator.deref_mut())
            ));
            vmas[index] = VirtualMemoryArea::new(vga_display_virt_addr, VGA_SIZE_IN_BYTES, vga_display_flags, "Kernel VGA Display Memory");
            debug!("mapped kernel section: vga_buffer at addr: {:?}", vmas[index]);
            // also do an identity mapping for APs that need it while booting
            identity_mapped_pages[index] = Some( try!( mapper.map_frames(
                FrameRange::from_phys_addr(PhysicalAddress::new(VGA_DISPLAY_PHYS_START)?, VGA_SIZE_IN_BYTES), 
                Page::containing_address(VirtualAddress::new_canonical(VGA_DISPLAY_PHYS_START)), 
                vga_display_flags, allocator.deref_mut())
            ));
            index += 1;
            

            // map the multiboot boot_info at the same address it previously was, so we can continue to access boot_info 
            let boot_info_pages  = PageRange::from_virt_addr(boot_info_start_vaddr, boot_info_size);
            let boot_info_frames = FrameRange::from_phys_addr(boot_info_start_paddr, boot_info_size);
            vmas[index] = VirtualMemoryArea::new(boot_info_start_vaddr, boot_info_size, EntryFlags::PRESENT | EntryFlags::GLOBAL, "Kernel Multiboot Info");
            for (page, frame) in boot_info_pages.into_iter().zip(boot_info_frames) {
                // we must do it page-by-page to make sure that a page hasn't already been mapped
                if mapper.translate_page(page).is_some() {
                    // skip pages that are already mapped
                    continue;
                }
                higher_half_mapped_pages[index] = Some( try!( mapper.map_to(
                    page, frame.clone(), EntryFlags::PRESENT | EntryFlags::GLOBAL, allocator.deref_mut())
                ));
                // also do an identity mapping, if maybe we need it?
                identity_mapped_pages[index] = Some( try!( mapper.map_to(
                    Page::containing_address(page.start_address() - KERNEL_OFFSET), frame, 
                    EntryFlags::PRESENT | EntryFlags::GLOBAL, allocator.deref_mut())
                ));
                index += 1;
            }

            debug!("identity_mapped_pages: {:?}", &identity_mapped_pages[0..(index + 1)]);

        } // unlocks the frame allocator 

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

    debug!("mapped and initialized the heap, VMA: {:?}", heap_vma);
    // HERE: now the heap is set up, we can use dynamically-allocated types like Vecs

    let mut kernel_vmas: Vec<VirtualMemoryArea> = vmas.to_vec();
    kernel_vmas.retain(|x|  *x != VirtualMemoryArea::default() );
    kernel_vmas.push(heap_vma);

    debug!("kernel_vmas: {:?}", kernel_vmas);

    let mut higher_half: Vec<MappedPages> = higher_half_mapped_pages.iter_mut().filter_map(|opt| opt.take()).collect();
    higher_half.push(heap_mapped_pages);
    let identity: Vec<MappedPages> = identity_mapped_pages.iter_mut().filter_map(|opt| opt.take()).collect();

    // Return the new_page_table because that's the one that should be used by the kernel in future mappings. 
    Ok((new_page_table, kernel_vmas, text_mapped_pages, rodata_mapped_pages, data_mapped_pages, higher_half, identity))
}