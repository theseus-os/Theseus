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
mod table;

pub use page_table_entry::PageTableEntry;

use crate::EarlyIdentityMappedPages;

pub use self::{
    temporary_page::TemporaryPage,
    mapper::{
        Mapper, MappedPages, BorrowedMappedPages, BorrowedSliceMappedPages,
        Mutability, Mutable, Immutable, translate,
    },
};

use core::{
    ops::{Deref, DerefMut},
    fmt,
};
use log::debug;
use super::{Frame, FrameRange, PageRange, VirtualAddress, PhysicalAddress,
    AllocatedPages, allocate_pages, AllocatedFrames, PteFlags,
    tlb_flush_all, tlb_flush_virt_addr, get_p4, find_section_memory_bounds,
    get_vga_mem_addr, InitialMemoryMappings
};
use pte_flags::PteFlagsArch;
use no_drop::NoDrop;
use boot_info::BootInformation;
use kernel_config::memory::{RECURSIVE_P4_INDEX, PAGE_SIZE, UPCOMING_PAGE_TABLE_RECURSIVE_P4_INDEX};


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
    /// An internal function to bootstrap a new top-level `PageTable` from
    /// the currently-active P4 frame (the root page table frame).
    /// 
    /// Returns an error if unable to allocate the `Frame` of the
    /// currently active page table root from the frame allocator.
    fn from_current() -> Result<PageTable, ()> {
        let current_p4 = frame_allocator::allocate_frames_at(get_current_p4().start_address(), 1)
            .map_err(|_| ())?;
    
        Ok(PageTable { 
            mapper: Mapper::with_p4_frame(*current_p4.start()),
            p4_table: current_p4,
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
            table[RECURSIVE_P4_INDEX].set_entry(
                frame.as_allocated_frame(),
                PteFlagsArch::new().valid(true).writable(true),
            );
        })?;

        let (_temp_page, inited_new_p4_frame) = temporary_page.unmap_into_parts(current_page_table)?;

        Ok(PageTable {
            mapper: Mapper::with_p4_frame(p4_frame),
            p4_table: inited_new_p4_frame.ok_or("BUG: PageTable::new_table(): failed to take back unmapped Frame for p4_table")?,
        })
    }

    /// Temporarily maps the given other `PageTable` to the temporary recursive
    /// index (508th entry)
    ///
    /// Accepts a closure `f` that is passed a mutable reference to the other
    /// table's mapper, and an immutable reference to the current table's
    /// mapper.
    ///
    /// # Note
    /// This does not perform any task switching or changing of the current page table register (e.g., cr3).
    pub fn with<F, R>(
        &mut self,
        other_table: &mut PageTable,
        f: F,
    ) -> Result<R, &'static str>
        where F: FnOnce(&mut Mapper, &Mapper) -> Result<R, &'static str>
    {
        let active_p4_frame = get_current_p4();
        if self.p4_table.start() != &active_p4_frame || self.p4_table.end() != &active_p4_frame {
            return Err("PageTable::with(): this PageTable ('self') must be the currently active page table.");
        }

        // Temporarily take ownership of the other page table's p4 allocated frame and
        // create a new temporary page that maps to that frame.
        let other_p4 = core::mem::replace(&mut other_table.p4_table, AllocatedFrames::empty());
        let other_p4_frame = *other_p4.start();
        let mut temporary_page = TemporaryPage::create_and_map_table_frame(None, other_p4, self)?;

        // Overwrite upcoming page table recursive mapping.
        temporary_page.with_table_and_frame(|table, frame| {
            self.p4_mut()[UPCOMING_PAGE_TABLE_RECURSIVE_P4_INDEX].set_entry(
                frame.as_allocated_frame(),
                PteFlagsArch::new().valid(true).writable(true),
            );
            table[UPCOMING_PAGE_TABLE_RECURSIVE_P4_INDEX].set_entry(
                frame.as_allocated_frame(),
                PteFlagsArch::new().valid(true).writable(true),
            );
        })?;
        tlb_flush_all();

        // This mapper will modify the `other_table` using the upcoming P4 recursive entry
        // that is set for the currently active page table.
        let mut mapper = Mapper::upcoming(other_p4_frame);

        // Execute `f` in the new context, in which the new page table is considered "active"
        let ret = f(&mut mapper, self);

        // Clear both page table's upcoming recursive mapping entries.
        self.p4_mut()[UPCOMING_PAGE_TABLE_RECURSIVE_P4_INDEX].zero();
        other_table.p4_mut()[UPCOMING_PAGE_TABLE_RECURSIVE_P4_INDEX].zero();
        tlb_flush_all();

        // Here, recover the other page table's p4 frame and restore it into the other page table,
        // since we removed it earlier at the top of this function and gave it to the temporary page. 
        let (_temp_page, p4_frame) = temporary_page.unmap_into_parts(self)?;
        other_table.p4_table = p4_frame.ok_or("BUG: PageTable::with(): failed to take back unmapped Frame for p4_table")?;

        ret
    }


    /// Switches from the currently-active page table (this `PageTable`, i.e., `self`) to the given `new_table`.
    /// After this function, the given `new_table` will be the currently-active `PageTable`.
    pub fn switch(&mut self, new_table: &PageTable) {
        // debug!("PageTable::switch() old table: {:?}, new table: {:?}", self, new_table);

        // perform the actual page table switch
        unsafe { 
            use x86_64::{PhysAddr, structures::paging::frame::PhysFrame, registers::control::{Cr3, Cr3Flags}};
            Cr3::write(
                PhysFrame::containing_address(PhysAddr::new_truncate(new_table.p4_table.start_address().value() as u64)),
                Cr3Flags::empty(),
            )
        };
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
pub fn init(
    boot_info: &impl BootInformation,
    stack_start_virt: VirtualAddress,
    into_alloc_frames_fn: fn(FrameRange) -> AllocatedFrames,
) -> Result<InitialMemoryMappings, &'static str> {
    // Store the callback from `frame_allocator::init()` that allows the `Mapper` to convert
    // `page_table_entry::UnmappedFrames` back into `AllocatedFrames`.
    mapper::INTO_ALLOCATED_FRAMES_FUNC.call_once(|| into_alloc_frames_fn);

    // bootstrap a PageTable from the currently-loaded page table
    let mut page_table = PageTable::from_current()
        .map_err(|_| "Failed to allocate frame for initial page table; is it merged with another section?")?;
    debug!("Bootstrapped initial {:?}", page_table);

    let (aggregated_section_memory_bounds, _sections_memory_bounds) = find_section_memory_bounds(boot_info, |virtual_address| page_table.translate(virtual_address))?;
    debug!("{:X?}\n{:X?}", aggregated_section_memory_bounds, _sections_memory_bounds);

    let boot_info_start_vaddr = boot_info.start().ok_or("boot_info start virtual address was invalid")?;
    let boot_info_start_paddr = page_table.translate(boot_info_start_vaddr).ok_or("Couldn't get boot_info start physical address")?;
    let boot_info_size = boot_info.len();
    debug!("multiboot vaddr: {:#X}, multiboot paddr: {:#X}, size: {:#X}", boot_info_start_vaddr, boot_info_start_paddr, boot_info_size);

    let new_p4_frame = frame_allocator::allocate_frames(1).ok_or("couldn't allocate frame for new page table")?; 
    let mut new_table = PageTable::new_table(&mut page_table, new_p4_frame, None)?;

    // Stack frames are not guaranteed to be contiguous in physical memory.
    let stack_size = boot_info.stack_size()?;
    let stack_page_range = PageRange::from_virt_addr(
        // `PAGE_SIZE` accounts for the guard page, which does not have a corresponding frame.
        stack_start_virt + PAGE_SIZE,
        stack_size - PAGE_SIZE,
    );
    debug!("Initial stack start {stack_start_virt:#X}, size: {stack_size:#X} bytes, {stack_page_range:X?}");

    // Boot info frames are not guaranteed to be contiguous in physical memory.
    let boot_info_page_range = PageRange::from_virt_addr(boot_info_start_vaddr, boot_info_size);
    debug!("Boot info start: {boot_info_start_vaddr:#X}, size: {boot_info_size:#X}, {boot_info_page_range:#X?}");

    // Create and initialize a new page table with the same contents as the currently-executing kernel.
    // This closure returns those new sections' mappings.
    let (
        text_mapped_pages,
        rodata_mapped_pages,
        data_mapped_pages,
        stack_page_group,
        boot_info_mapped_pages,
        identity_mapped_pages,
        additional_mapped_pages,
    ) = page_table.with(&mut new_table, |new_mapper, current_mapper| {
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

        let (init_start_virt,    init_start_phys)    = aggregated_section_memory_bounds.init.start;
        let (init_end_virt,      init_end_phys)      = aggregated_section_memory_bounds.init.end;
        let (text_start_virt,    text_start_phys)    = aggregated_section_memory_bounds.text.start;
        let (text_end_virt,      text_end_phys)      = aggregated_section_memory_bounds.text.end;
        let (rodata_start_virt,  rodata_start_phys)  = aggregated_section_memory_bounds.rodata.start;
        let (rodata_end_virt,    rodata_end_phys)    = aggregated_section_memory_bounds.rodata.end;
        let (data_start_virt,    data_start_phys)    = aggregated_section_memory_bounds.data.start;
        let (data_end_virt,      data_end_phys)      = aggregated_section_memory_bounds.data.end;

        let init_flags    = aggregated_section_memory_bounds.init.flags;
        let text_flags    = aggregated_section_memory_bounds.text.flags;
        let rodata_flags  = aggregated_section_memory_bounds.rodata.flags;
        let data_flags    = aggregated_section_memory_bounds.data.flags;

        let mut boot_info_mapped_pages:    Option<MappedPages> = None;

        let init_pages = page_allocator::allocate_pages_by_bytes_at(init_start_virt, init_end_virt.value() - init_start_virt.value())?;
        let init_frames = frame_allocator::allocate_frames_by_bytes_at(init_start_phys, init_end_phys.value() - init_start_phys.value())?;
        let init_pages_identity = page_allocator::allocate_pages_by_bytes_at(
            VirtualAddress::new_canonical(init_start_phys.value()),
            init_end_phys.value() - init_start_phys.value(),
        )?;
        let init_identity_mapped_pages: NoDrop<MappedPages> = NoDrop::new( unsafe {
            Mapper::map_to_non_exclusive(new_mapper, init_pages_identity, &init_frames, init_flags)?
        });
        let mut init_mapped_pages = new_mapper.map_allocated_pages_to(init_pages, init_frames, init_flags)?;

        let text_pages = page_allocator::allocate_pages_by_bytes_at(text_start_virt, text_end_virt.value() - text_start_virt.value())?;
        let text_frames = frame_allocator::allocate_frames_by_bytes_at(text_start_phys, text_end_phys.value() - text_start_phys.value())?;
        let text_pages_identity = page_allocator::allocate_pages_by_bytes_at(
            VirtualAddress::new_canonical(text_start_phys.value()),
            text_end_phys.value() - text_start_phys.value(),
        )?;
        let text_identity_mapped_pages: NoDrop<MappedPages> = NoDrop::new( unsafe {
            Mapper::map_to_non_exclusive(new_mapper, text_pages_identity, &text_frames, text_flags)?
        });
        init_mapped_pages.merge(new_mapper.map_allocated_pages_to(text_pages, text_frames, text_flags)?).map_err(|(error, _)| error)?;
        let text_mapped_pages = NoDrop::new(init_mapped_pages);

        let rodata_pages = page_allocator::allocate_pages_by_bytes_at(rodata_start_virt, rodata_end_virt.value() - rodata_start_virt.value())?;
        let rodata_frames = frame_allocator::allocate_frames_by_bytes_at(rodata_start_phys, rodata_end_phys.value() - rodata_start_phys.value())?;
        let rodata_pages_identity = page_allocator::allocate_pages_by_bytes_at(
            VirtualAddress::new_canonical(rodata_start_phys.value()),
            rodata_end_phys.value() - rodata_start_phys.value(),
        )?;
        let rodata_identity_mapped_pages: NoDrop<MappedPages> = NoDrop::new( unsafe {
            Mapper::map_to_non_exclusive(new_mapper, rodata_pages_identity, &rodata_frames, rodata_flags)?
        });
        let rodata_mapped_pages = NoDrop::new(new_mapper.map_allocated_pages_to(rodata_pages, rodata_frames, rodata_flags)?);

        let data_pages = page_allocator::allocate_pages_by_bytes_at(data_start_virt, data_end_virt.value() - data_start_virt.value())?;
        let data_frames = frame_allocator::allocate_frames_by_bytes_at(data_start_phys, data_end_phys.value() - data_start_phys.value())?;
        let data_pages_identity = page_allocator::allocate_pages_by_bytes_at(
            VirtualAddress::new_canonical(data_start_phys.value()),
            data_end_phys.value() - data_start_phys.value(),
        )?;
        let data_identity_mapped_pages: NoDrop<MappedPages> = NoDrop::new( unsafe {
            Mapper::map_to_non_exclusive(new_mapper, data_pages_identity, &data_frames, data_flags)?
        });
        let data_mapped_pages = NoDrop::new(new_mapper.map_allocated_pages_to(data_pages, data_frames, data_flags)?);

        // Handle the stack (a separate data section), which consists of one guard page (unmapped)
        // followed by the real (mapped) stack pages.
        // The stack does not need to be identity mapped, because each secondary CPU will get its own stack.
        let stack_guard_page = page_allocator::allocate_pages_at(stack_start_virt, 1)?;
        let mut stack_mapped_pages: Option<MappedPages> = None;
        for page in stack_page_range.into_iter() {
            // The stack is not guaranteed to be contiguous in physical memory,
            // so we use the `current_mapper` to translate each page into its backing physical frame,
            // and then reproduce the same mapping in the `new_mapper`.
            let frame = current_mapper.translate_page(page).ok_or("couldn't translate stack page")?;
            let allocated_page = page_allocator::allocate_pages_at(page.start_address(), 1)?;
            let allocated_frame = frame_allocator::allocate_frames_at(frame.start_address(), 1)?;
            let mapped_pages = new_mapper.map_allocated_pages_to(allocated_page, allocated_frame, data_flags)?;
            if let Some(ref mut stack_mapped_pages) = stack_mapped_pages {
                stack_mapped_pages.merge(mapped_pages).map_err(|_| "failed to merge stack mapped pages")?;
            } else {
                stack_mapped_pages = Some(mapped_pages);
            }
        }
        let stack_page_group = (
            stack_guard_page,
            NoDrop::new(stack_mapped_pages.ok_or("no pages were allocated for the stack")?),
        );

        // Map the VGA display memory as writable. 
        // We map this *only* using an identity mapping, as it is only used during early init
        // by both the bootstrap CPU and secondary CPUs.
        let (vga_phys_addr, vga_size_in_bytes, vga_flags) = get_vga_mem_addr()?;
        let vga_display_pages_identity = page_allocator::allocate_pages_by_bytes_at(
            VirtualAddress::new_canonical(vga_phys_addr.value()),
            vga_size_in_bytes,
        )?;
        let vga_display_frames = frame_allocator::allocate_frames_by_bytes_at(vga_phys_addr, vga_size_in_bytes)?;
        let additional_mapped_pages: NoDrop<MappedPages> = NoDrop::new(new_mapper.map_allocated_pages_to(
            vga_display_pages_identity, vga_display_frames, vga_flags,
        )?);

        // Map the bootloader info, a separate region of read-only memory, so that we can access it later.
        // This does not need to be identity mapped.
        for page in boot_info_page_range.into_iter() {
            // The boot info is not guaranteed to be contiguous in physical memory,
            // so we use the `current_mapper` to translate each page into its backing physical frame,
            // and then reproduce the same mapping in the `new_mapper`.
            let frame = current_mapper.translate_page(page).ok_or("couldn't translate stack page")?;
            let allocated_page = page_allocator::allocate_pages_at(page.start_address(), 1)?;
            let allocated_frame = frame_allocator::allocate_frames_at(frame.start_address(), 1)?;
            let mapped_pages = new_mapper.map_allocated_pages_to(allocated_page, allocated_frame, PteFlags::new())?;
            if let Some(ref mut boot_info_mp) = boot_info_mapped_pages {
                boot_info_mp.merge(mapped_pages).map_err(|_| "failed to merge boot info pages")?;
            } else {
                boot_info_mapped_pages = Some(mapped_pages);
            }
        }

        let identity_mapped_pages = NoDrop::new(EarlyIdentityMappedPages {
            _init:   init_identity_mapped_pages.into_inner(),
            _text:   text_identity_mapped_pages.into_inner(),
            _rodata: rodata_identity_mapped_pages.into_inner(),
            _data:   data_identity_mapped_pages.into_inner(),
        });
        debug!("{identity_mapped_pages:?}");
        debug!("{additional_mapped_pages:?}");
        Ok((
            text_mapped_pages,
            rodata_mapped_pages,
            data_mapped_pages,
            stack_page_group,
            boot_info_mapped_pages,
            identity_mapped_pages,
            additional_mapped_pages,
        ))
    })?;

    let boot_info_mapped_pages  = boot_info_mapped_pages.ok_or("Couldn't map boot_info pages section")?;

    debug!("switching from old page table {:?} to new page table {:?}", page_table, new_table);
    page_table.switch(&new_table); 
    debug!("switched to new page table {:?}.", new_table); 
    // The old page_table set up during bootstrap will be dropped here. It's no longer being used.

    // Return the new page table because that's the one that should be used by the kernel in future mappings. 
    Ok(InitialMemoryMappings {
        page_table: new_table,
        text: text_mapped_pages,
        rodata: rodata_mapped_pages,
        data: data_mapped_pages,
        stack_guard: stack_page_group.0,
        stack: stack_page_group.1,
        boot_info: boot_info_mapped_pages,
        identity: identity_mapped_pages,
        additional: additional_mapped_pages,
    })
}
