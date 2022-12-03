// Copyright 2016 Philipp Oppermann. See the README.md
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

mod temporary_page;
mod mapper;
#[cfg(not(mapper_spillful))]
mod table;
#[cfg(mapper_spillful)]
pub mod table;


pub use page_table_entry::PageTableEntry;
pub use self::{
    temporary_page::TemporaryPage,
    mapper::{
        Mapper, MappedPages, BorrowedMappedPages, BorrowedSliceMappedPages,
        Mutability, Mutable, Immutable,
    },
};

use core::{
    ops::{Deref, DerefMut},
    fmt,
};
use super::{Frame, FrameRange, Page, PageRange, VirtualAddress, PhysicalAddress,
    AllocatedPages, allocate_pages, AllocatedFrames, EntryFlags,
    tlb_flush_all, tlb_flush_virt_addr, get_p4, find_section_memory_bounds,
    get_vga_mem_addr, KERNEL_OFFSET};
use no_drop::NoDrop;
use kernel_config::memory::{RECURSIVE_P4_INDEX};
// use kernel_config::memory::{KERNEL_TEXT_P4_INDEX, KERNEL_HEAP_P4_INDEX, KERNEL_STACK_P4_INDEX};


/// A top-level root (P4) page table.
/// 
/// Auto-derefs into a `Mapper` for easy invocation of memory mapping functions.
pub struct PageTable {
    mapper: Mapper,
    p4_table: AllocatedFrames,
}
impl fmt::Debug for PageTable {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "PageTable(p4: {:#X})", self.p4_table.start_address()) 
    }
}

impl Deref for PageTable {
    type Target = Mapper;

    fn deref(&self) -> &Mapper {
        &self.mapper
    }
}

impl DerefMut for PageTable {
    fn deref_mut(&mut self) -> &mut Mapper {
        &mut self.mapper
    }
}

impl PageTable {
    /// An internal function to bootstrap a new top-level PageTable 
    /// based on the given currently-active P4 frame (the frame holding the page table root).
    /// 
    /// Returns an error if the given `active_p4_frame` is not the currently active page table.
    fn from_current(active_p4_frame: AllocatedFrames) -> Result<PageTable, &'static str> {
        assert!(active_p4_frame.size_in_frames() == 1);
        let current_p4 = get_current_p4();
        if active_p4_frame.start() != &current_p4 {
            return Err("PageTable::from_current(): the active_p4_frame must be the root of the currently-active page table.");
        }
        Ok(PageTable { 
            mapper: Mapper::with_p4_frame(current_p4),
            p4_table: active_p4_frame,
        })
    }

    /// Initializes a new top-level P4 `PageTable` whose root is located in the given `new_p4_frame`.
    /// It requires using the given `current_active_table` to set up its initial mapping contents.
    /// 
    /// A single allocated page can optionally be provided for use as part of a new `TemporaryPage`
    /// for the recursive mapping.
    /// 
    /// Returns the new `PageTable` that exists in physical memory at the given `new_p4_frame`. 
    /// Note that this new page table has no current mappings beyond the recursive P4 mapping,
    /// so you will need to create or copy over any relevant mappings 
    /// before using (switching) to this new page table in order to ensure the system keeps running.
    pub fn new_table(
        current_page_table: &mut PageTable,
        new_p4_frame: AllocatedFrames,
        page: Option<AllocatedPages>,
    ) -> Result<PageTable, &'static str> {
        let p4_frame = *new_p4_frame.start();

        let mut temporary_page = TemporaryPage::create_and_map_table_frame(page, new_p4_frame, current_page_table)?;
        temporary_page.with_table_and_frame(|table, frame| {
            table.zero();
            table[RECURSIVE_P4_INDEX].set_entry(frame.as_allocated_frame(), EntryFlags::PRESENT | EntryFlags::WRITABLE);
        })?;

        let (_temp_page, inited_new_p4_frame) = temporary_page.unmap_into_parts(current_page_table)?;

        Ok(PageTable {
            mapper: Mapper::with_p4_frame(p4_frame),
            p4_table: inited_new_p4_frame.ok_or("BUG: PageTable::new_table(): failed to take back unmapped Frame for p4_table")?,
        })
    }

    /// Temporarily maps the given other `PageTable` to the recursive entry (510th entry) 
    /// so that the given closure `f` can set up new mappings on the new `other_table` without actually switching to it yet.
    /// Accepts a closure `f` that is passed  a `Mapper`, such that it can set up new mappings on the other table.
    /// Consumes the given `temporary_page` and automatically unmaps it afterwards. 
    /// # Note
    /// This does not perform any task switching or changing of the current page table register (e.g., cr3).
    pub fn with<F>(
        &mut self,
        other_table: &mut PageTable,
        f: F,
    ) -> Result<(), &'static str>
        where F: FnOnce(&mut Mapper) -> Result<(), &'static str>
    {
        let active_p4_frame = get_current_p4();
        if self.p4_table.start() != &active_p4_frame || self.p4_table.end() != &active_p4_frame {
            return Err("PageTable::with(): this PageTable ('self') must be the currently active page table.");
        }

        // Temporarily take ownership of this page table's p4 allocated frame and
        // create a new temporary page that maps to that frame.
        let this_p4 = core::mem::replace(&mut self.p4_table, AllocatedFrames::empty());
        let mut temporary_page = TemporaryPage::create_and_map_table_frame(None, this_p4, self)?;

        // overwrite recursive mapping
        self.p4_mut()[RECURSIVE_P4_INDEX].set_entry(other_table.p4_table.as_allocated_frame(), EntryFlags::PRESENT | EntryFlags::WRITABLE); 
        tlb_flush_all();

        // set mapper's target frame to reflect that future mappings will be mapped into the other_table
        self.mapper.target_p4 = *other_table.p4_table.start();

        // execute `f` in the new context, in which the new page table is considered "active"
        let ret = f(self);

        // restore mapper's target frame to reflect that future mappings will be mapped using the currently-active (original) PageTable
        self.mapper.target_p4 = active_p4_frame;

        // restore recursive mapping to original p4 table
        temporary_page.with_table_and_frame(|p4_table, frame| {
            p4_table[RECURSIVE_P4_INDEX].set_entry(frame.as_allocated_frame(), EntryFlags::PRESENT | EntryFlags::WRITABLE);
        })?;
        tlb_flush_all();

        // Here, recover the current page table's p4 frame and restore it into this current page table,
        // since we removed it earlier at the top of this function and gave it to the temporary page. 
        let (_temp_page, p4_frame) = temporary_page.unmap_into_parts(self)?;
        self.p4_table = p4_frame.ok_or("BUG: PageTable::with(): failed to take back unmapped Frame for p4_table")?;

        ret
    }


    /// Switches from the currently-active page table (this `PageTable`, i.e., `self`) to the given `new_table`.
    /// After this function, the given `new_table` will be the currently-active `PageTable`.
    pub fn switch(&mut self, new_table: &PageTable) {
        // use core::fmt::Write;
        debug!("PageTable::switch() old table: {:?}, new table: {:?}", self, new_table);
        // let mut port = serial_port_basic::SerialPort::new(0x3f8);

        // perform the actual page table switch
        unsafe { 
            use x86_64::{PhysAddr, structures::paging::frame::PhysFrame, registers::control::{Cr3, Cr3Flags}};
            Cr3::write(
                PhysFrame::containing_address(PhysAddr::new_truncate(new_table.p4_table.start_address().value() as u64)),
                Cr3Flags::empty(),
            )
        };
        // let u = logger::take_early_log_writers();
        // write!(port, "logger after: {:?}", u).unwrap();
        debug!("won't work");
        
        // let addr = new_table.p4_table.start_address().value() as u64;
        // unsafe { core::arch::asm!("mov r10, 0xdeadc0de") };
    }


    /// Returns the physical address of this page table's top-level p4 frame
    pub fn physical_address(&self) -> PhysicalAddress {
        self.p4_table.start_address()
    }
}


/// Returns the current top-level (P4) root page table frame.
pub fn get_current_p4() -> Frame {
    Frame::containing_address(get_p4())
}


/// Initializes a new page table and sets up all necessary mappings for the kernel to continue running. 
/// Returns the following tuple, if successful:
/// 
///  1. The kernel's new PageTable, which is now currently active,
///  2. the kernel's text section MappedPages,
///  3. the kernel's rodata section MappedPages,
///  4. the kernel's data section MappedPages,
///  5. a tuple of the stack's underlying guard page (an `AllocatedPages` instance) and the actual `MappedPages` backing it,
///  6. the `MappedPages` holding the bootloader info,
///  7. the kernel's list of *other* higher-half MappedPages that needs to be converted to a vector after heap initialization, and which should be kept forever,
///  8. the kernel's list of identity-mapped MappedPages that needs to be converted to a vector after heap initialization, and which should be dropped before starting the first userspace program. 
///
/// Otherwise, it returns a str error message. 
pub fn init<T>(
    boot_info: &T,
    into_alloc_frames_fn: fn(FrameRange) -> AllocatedFrames,
) -> Result<(
        PageTable,
        NoDrop<MappedPages>,
        NoDrop<MappedPages>,
        NoDrop<MappedPages>,
        (AllocatedPages, NoDrop<MappedPages>),
        MappedPages,
        [Option<NoDrop<MappedPages>>; 32],
        [Option<NoDrop<MappedPages>>; 32],
    ), &'static str>
where
    T: boot_info::BootInformation
{
    // Store the callback from `frame_allocator::init()` that allows the `Mapper` to convert
    // `page_table_entry::UnmappedFrames` back into `AllocatedFrames`.
    mapper::INTO_ALLOCATED_FRAMES_FUNC.call_once(|| into_alloc_frames_fn);

    let (aggregated_section_memory_bounds, _sections_memory_bounds) = find_section_memory_bounds(boot_info)?;
    debug!("{:X?}\n{:X?}", aggregated_section_memory_bounds, _sections_memory_bounds);
    
    // bootstrap a PageTable from the currently-loaded page table
    // aggregated_section_memory_bounds.page_table.start.1
    let page_table_start: usize;
    unsafe { core::arch::asm!("mov {}, cr3", out(reg) page_table_start) };
    let current_active_p4 = frame_allocator::allocate_frames_at(PhysicalAddress::new(page_table_start).unwrap(), 1)?;
    let mut page_table = PageTable::from_current(current_active_p4)?;
    debug!("Bootstrapped initial {:?}", page_table);

    // FIXME: Niceify
    // let boot_info_start_vaddr = VirtualAddress::new(boot_info.bootloader_info_memory_range()?.start.value() + KERNEL_OFFSET).ok_or("boot_info start virtual address was invalid")?;
    // let boot_info_start_vaddr = VirtualAddress::new(boot_info.bootloader_info_memory_range()?.start.value()).ok_or("boot_info start virtual address was invalid")?;
    // let boot_info_start_vaddr = VirtualAddress::new(boot_info as *const _ as usize).unwrap();
    let boot_info_start_vaddr = boot_info.start();
    let boot_info_start_paddr = page_table.translate(boot_info_start_vaddr).ok_or("Couldn't get boot_info start physical address")?;
    let boot_info_size = boot_info.size();
    debug!("multiboot vaddr: {:#X}, multiboot paddr: {:#X}, size: {:#X}", boot_info_start_vaddr, boot_info_start_paddr, boot_info_size);

    let new_p4_frame = frame_allocator::allocate_frames(1).ok_or("couldn't allocate frame for new page table")?; 
    let mut new_table = PageTable::new_table(&mut page_table, new_p4_frame, None)?;

    let mut text_mapped_pages:        Option<NoDrop<MappedPages>> = None;
    let mut rodata_mapped_pages:      Option<NoDrop<MappedPages>> = None;
    let mut data_mapped_pages:        Option<NoDrop<MappedPages>> = None;
    let mut stack_page_group:         Option<(AllocatedPages, NoDrop<MappedPages>)> = None;
    let mut boot_info_mapped_pages:   Option<MappedPages> = None;
    let mut higher_half_mapped_pages: [Option<NoDrop<MappedPages>>; 32] = Default::default();
    let mut identity_mapped_pages:    [Option<NoDrop<MappedPages>>; 32] = Default::default();

    let (text_start_virt,    text_start_phys)    = aggregated_section_memory_bounds.text.start;
    let (text_end_virt,      text_end_phys)      = aggregated_section_memory_bounds.text.end;
    let (rodata_start_virt,  rodata_start_phys)  = aggregated_section_memory_bounds.rodata.start;
    let (rodata_end_virt,    rodata_end_phys)    = aggregated_section_memory_bounds.rodata.end;
    let (data_start_virt,    data_start_phys)    = aggregated_section_memory_bounds.data.start;
    let (data_end_virt,      data_end_phys)      = aggregated_section_memory_bounds.data.end;
    let (stack_start_virt,   stack_start_phys)   = aggregated_section_memory_bounds.stack.start;
    let (stack_end_virt,     _stack_end_phys)    = aggregated_section_memory_bounds.stack.end;

    let text_flags    = aggregated_section_memory_bounds.text.flags;
    let rodata_flags  = aggregated_section_memory_bounds.rodata.flags;
    let data_flags    = aggregated_section_memory_bounds.data.flags;

    /// Stack frames are not guaranteed to be contiguous.
    let mut stack_mappings = [None; 20];
    // + 1 accounts for the guard page.
    for (i, page) in ((Page::containing_address(stack_start_virt) + 1)..=Page::containing_address(stack_end_virt - 1)).enumerate() {
        let frame = page_table.translate_page(page).expect("couldn't translate stack page");
        stack_mappings[i] = Some((page, frame));
    }
    
    /// Boot info frames are not guaranteed to be contiguous.
    let mut boot_info_mappings = [None; 10];
    for (i, page) in (Page::containing_address(boot_info_start_vaddr)..=Page::containing_address(boot_info_start_vaddr + boot_info_size - 1)).enumerate() {
        let frame = page_table.translate_page(page).expect("couldn't translate stack page");
        boot_info_mappings[i] = Some((page, frame));
    }

    // Create and initialize a new page table with the same contents as the currently-executing kernel code/data sections.
    page_table.with(&mut new_table, |mapper| {
        
        // Map every section found in the kernel image (given by the boot information above) into our new page table. 
        // To allow the APs to boot up, we must identity map those kernel sections too, i.e., 
        // map the same physical frames to both lower-half and higher-half virtual addresses. 
        // This is the only time in Theseus that we permit non-bijective (non 1-to-1) virtual-to-physical memory mappings,
        // since it is unavoidable if we want to place the kernel in the higher half. 
        // Debatably, this is no longer needed because we're don't have a userspace, and there's no real reason to 
        // place the kernel in the higher half. 
        //
        // These identity mappings are short-lived; they are unmapped later after all other CPUs are brought up
        // but before we start running applications.

        debug!("{:X?}", aggregated_section_memory_bounds);
        let mut index = 0;

        log::trace!("text virt {:0x?} {:0x?}", text_start_virt.value(), text_end_virt.value());
        log::trace!("text phys {:0x?} {:0x?}", text_start_phys.value(), text_end_phys.value());

        let text_pages = page_allocator::allocate_pages_by_bytes_at(text_start_virt, text_end_virt.value() - text_start_virt.value())?;
        let text_frames = frame_allocator::allocate_frames_by_bytes_at(text_start_phys, text_end_phys.value() - text_start_phys.value())?;
        let text_pages_identity = page_allocator::allocate_pages_by_bytes_at(text_start_virt - KERNEL_OFFSET, text_end_virt.value() - text_start_virt.value())?;
        identity_mapped_pages[index] = Some(NoDrop::new( unsafe {
            Mapper::map_to_non_exclusive(mapper, text_pages_identity, &text_frames, text_flags)?
        }));
        text_mapped_pages = Some(NoDrop::new(mapper.map_allocated_pages_to(text_pages, text_frames, text_flags)?));
        index += 1;

        log::trace!("allocating rodata pages");
        let rodata_pages = page_allocator::allocate_pages_by_bytes_at(rodata_start_virt, rodata_end_virt.value() - rodata_start_virt.value())?;
        let rodata_frames = frame_allocator::allocate_frames_by_bytes_at(rodata_start_phys, rodata_end_phys.value() - rodata_start_phys.value())?;
        let rodata_pages_identity = page_allocator::allocate_pages_by_bytes_at(rodata_start_virt - KERNEL_OFFSET, rodata_end_virt.value() - rodata_start_virt.value())?;
        identity_mapped_pages[index] = Some(NoDrop::new( unsafe {
            Mapper::map_to_non_exclusive(mapper, rodata_pages_identity, &rodata_frames, rodata_flags)?
        }));
        rodata_mapped_pages = Some(NoDrop::new(mapper.map_allocated_pages_to(rodata_pages, rodata_frames, rodata_flags)?));
        index += 1;

        log::trace!("allocating data pages");
        let data_pages = page_allocator::allocate_pages_by_bytes_at(data_start_virt, data_end_virt.value() - data_start_virt.value())?;
        let data_frames = frame_allocator::allocate_frames_by_bytes_at(data_start_phys, data_end_phys.value() - data_start_phys.value())?;
        let data_pages_identity = page_allocator::allocate_pages_by_bytes_at(data_start_virt - KERNEL_OFFSET, data_end_virt.value() - data_start_virt.value())?;
        identity_mapped_pages[index] = Some(NoDrop::new( unsafe {
            Mapper::map_to_non_exclusive(mapper, data_pages_identity, &data_frames, data_flags)?
        }));
        data_mapped_pages = Some(NoDrop::new(mapper.map_allocated_pages_to(data_pages, data_frames, data_flags)?));
        index += 1;

        // We don't need to do any mapping for the initial root (P4) page table stack (a separate data section),
        // which was initially set up and created by the bootstrap assembly code. 
        // It was used to bootstrap the initial page table at the beginning of this function. 

        log::trace!("allocating stack pages");
        // Handle the stack (a separate data section), which consists of one guard page followed by the real stack pages.
        // It does not need to be identity mapped because each AP core will have its own stack.
        let stack_guard_page = page_allocator::allocate_pages_at(stack_start_virt, 1).unwrap();
        let mut stack_mapped_pages: Option<MappedPages> = None;
        let mut iter = stack_mappings.iter();
        while let Some(Some((page, frame))) = iter.next() {
            // log::info!("mapping stack page: {page:0x?} {frame:0x?}");
            let allocated_page = page_allocator::allocate_pages_at(page.start_address(), 1).unwrap();
            let allocated_frame = frame_allocator::allocate_frames_at(frame.start_address(), 1).unwrap();
            let mapped_pages = mapper.map_allocated_pages_to(allocated_page, allocated_frame, data_flags)?;
            if let Some(ref mut stack_mapped_pages) = stack_mapped_pages {
                stack_mapped_pages.merge(mapped_pages).unwrap();
            } else {
                stack_mapped_pages = Some(mapped_pages);
            }
        }
        stack_page_group = Some((stack_guard_page, NoDrop::new(stack_mapped_pages.unwrap())));

        log::trace!("allocating vga pages");
        // Map the VGA display memory as writable.
        // We do an identity mapping for the VGA display too, because the AP cores may access it while booting.
        let (vga_phys_addr, vga_size_in_bytes, vga_flags) = get_vga_mem_addr()?;
        let vga_virt_addr_identity = VirtualAddress::new_canonical(vga_phys_addr.value());
        let vga_display_pages = page_allocator::allocate_pages_by_bytes_at(vga_virt_addr_identity + KERNEL_OFFSET, vga_size_in_bytes)?;
        let vga_display_frames = frame_allocator::allocate_frames_by_bytes_at(vga_phys_addr, vga_size_in_bytes)?;
        let vga_display_pages_identity = page_allocator::allocate_pages_by_bytes_at(vga_virt_addr_identity, vga_size_in_bytes)?;
        identity_mapped_pages[index] = Some(NoDrop::new( unsafe {
            Mapper::map_to_non_exclusive(mapper, vga_display_pages_identity, &vga_display_frames, vga_flags)?
        }));
        higher_half_mapped_pages[index] = Some(NoDrop::new(mapper.map_allocated_pages_to(vga_display_pages, vga_display_frames, vga_flags)?));
        index += 1;


        let mut iter = boot_info_mappings.iter();
        while let Some(Some((page, frame))) = iter.next() {
            let allocated_page = page_allocator::allocate_pages_at(page.start_address(), 1).unwrap();
            let allocated_frame = frame_allocator::allocate_frames_at(frame.start_address(), 1).unwrap();
            let mapped_pages = mapper.map_allocated_pages_to(allocated_page, allocated_frame, data_flags)?;
            if let Some(ref mut boot_info_mapped_pages) = boot_info_mapped_pages {
                boot_info_mapped_pages.merge(mapped_pages).unwrap();
            } else {
                boot_info_mapped_pages = Some(mapped_pages);
            }
        }

        debug!("identity_mapped_pages: {:?}", &identity_mapped_pages[..index]);
        debug!("higher_half_mapped_pages: {:?}", &higher_half_mapped_pages[..index]);

        Ok(()) // mapping closure completed successfully

    })?; // TemporaryPage is dropped here


    let text_mapped_pages       = text_mapped_pages     .ok_or("Couldn't map .text section")?;
    let rodata_mapped_pages     = rodata_mapped_pages   .ok_or("Couldn't map .rodata section")?;
    let data_mapped_pages       = data_mapped_pages     .ok_or("Couldn't map .data section")?;
    let boot_info_mapped_pages  = boot_info_mapped_pages.ok_or("Couldn't map boot_info pages section")?;
    let stack_page_group        = stack_page_group      .ok_or("Couldn't map .stack section")?;

    debug!("switching from old page table {:?} to new page table {:?}", page_table, new_table);
    let rip = &logger::EARLY_LOGGER as *const _ as usize;
    debug!("rip: {rip:0x?}");
    // let rip: usize;
    // unsafe { core::arch::asm!("mov {}, rsp", out(reg) rip) };
    // let rip = 0xffffffff802682b0;
    // let rip = port_io::Port::<u8>::new as usize;
    let old_phys = page_table.translate(VirtualAddress::new(rip).unwrap()).unwrap();
    let new_phys = new_table.translate(VirtualAddress::new(rip).unwrap()).unwrap();
    debug!("old_phys: {old_phys:p}");
    debug!("new_phys: {new_phys:p}");
    // debug!("early: {:#?}", EARLY_LOGGER);
    page_table.switch(&new_table); 
    // unsafe { core::arch::asm!("mov r8, 0xa") };
    debug!("switched to new page table {:?}.", new_table); 
    // unsafe { core::arch::asm!("mov r9, 0xaaa") };
    // let mut port = serial_port_basic::SerialPort::new(0x3f8);
    // port.write_fmt(format_args!("i'm here {:#?}", EARLY_LOGGER));
    // The old page_table set up during bootstrap will be dropped here. It's no longer being used.
    
    // Return the new page table because that's the one that should be used by the kernel in future mappings. 
    Ok((
        new_table,
        text_mapped_pages,
        rodata_mapped_pages,
        data_mapped_pages,
        stack_page_group,
        boot_info_mapped_pages,
        higher_half_mapped_pages,
        identity_mapped_pages
    ))
}
