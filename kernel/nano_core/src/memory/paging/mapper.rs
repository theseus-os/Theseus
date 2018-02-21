// Copyright 2016 Philipp Oppermann. See the README.md
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use x86_64;
use super::super::*; //{VirtualAddress, PhysicalAddress, Page, ENTRIES_PER_PAGE_TABLE};
use super::table::{self, Table, Level4};
use memory::{Frame, FrameAllocator, AllocatedPages};
use core::ptr::Unique;
use kernel_config::memory::{ENTRIES_PER_PAGE_TABLE, PAGE_SIZE, TEMPORARY_PAGE_VIRT_ADDR};

pub struct Mapper {
    p4: Unique<Table<Level4>>,
    pub target_p4: Frame,
}

impl Mapper {
    pub unsafe fn new() -> Mapper {
        Mapper { 
            p4: Unique::new_unchecked(table::P4),
            target_p4: get_current_p4(),
        }
    }

    pub fn p4(&self) -> &Table<Level4> {
        unsafe { self.p4.as_ref() }
    }

    pub fn p4_mut(&mut self) -> &mut Table<Level4> {
        unsafe { self.p4.as_mut() }
    }

    /// translates a VirtualAddress to a PhysicalAddress
    pub fn translate(&self, virtual_address: VirtualAddress) -> Option<PhysicalAddress> {
        let offset = virtual_address % PAGE_SIZE;
        // get the frame number of the page containing the given virtual address,
        // and then the corresponding physical address is that PFN*sizeof(Page) + offset
        self.translate_page(Page::containing_address(virtual_address)).map(|frame| {
            frame.number * PAGE_SIZE + offset
        })
    }

    pub fn translate_page(&self, page: Page) -> Option<Frame> {
        let p3 = self.p4().next_table(page.p4_index());

        let huge_page = || {
            p3.and_then(|p3| {
                let p3_entry = &p3[page.p3_index()];
                // 1GiB page?
                if let Some(start_frame) = p3_entry.pointed_frame() {
                    if p3_entry.flags().contains(EntryFlags::HUGE_PAGE) {
                        // address must be 1GiB aligned
                        assert!(start_frame.number % (ENTRIES_PER_PAGE_TABLE * ENTRIES_PER_PAGE_TABLE) == 0);
                        return Some(Frame {
                            number: start_frame.number + page.p2_index() * ENTRIES_PER_PAGE_TABLE + page.p1_index(),
                        });
                    }
                }
                if let Some(p2) = p3.next_table(page.p3_index()) {
                    let p2_entry = &p2[page.p2_index()];
                    // 2MiB page?
                    if let Some(start_frame) = p2_entry.pointed_frame() {
                        if p2_entry.flags().contains(EntryFlags::HUGE_PAGE) {
                            // address must be 2MiB aligned
                            assert!(start_frame.number % ENTRIES_PER_PAGE_TABLE == 0);
                            return Some(Frame { number: start_frame.number + page.p1_index() });
                        }
                    }
                }
                None
            })
        };

        p3.and_then(|p3| p3.next_table(page.p3_index()))
            .and_then(|p2| p2.next_table(page.p2_index()))
            .and_then(|p1| p1[page.p1_index()].pointed_frame())
            .or_else(huge_page)
    }


    /// the internal function that actually does the mapping, if frames were NOT provided.
    fn internal_map<A>(&mut self, pages: PageIter, flags: EntryFlags, allocator: &mut A)
        -> Result<MappedPages, &'static str>
        where A: FrameAllocator
    {
        for page in pages.clone() {
            let frame = try!(allocator.allocate_frame().ok_or("map_internal(): couldn't allocate new frame, out of memory!"));

            let mut p3 = self.p4_mut().next_table_create(page.p4_index(), flags, allocator);
            let mut p2 = p3.next_table_create(page.p3_index(), flags, allocator);
            let mut p1 = p2.next_table_create(page.p2_index(), flags, allocator);

            if !p1[page.p1_index()].is_unused() {
                error!("map_to() page {:#x} -> frame {:#X}, page was already in use!", page.start_address(), frame.start_address());
                return Err("page was already in use");
            } 

            p1[page.p1_index()].set(frame, flags | EntryFlags::PRESENT);
        }

        Ok(MappedPages {
            page_table_p4: self.target_p4.clone(),
            pages: pages,
            allocated: None,
        })
    }

    /// the internal function that actually does all of the mapping from pages to frames.
    fn internal_map_to<A>(&mut self, pages: PageIter, frames: FrameIter, flags: EntryFlags, allocator: &mut A)
        -> Result<MappedPages, &'static str>
        where A: FrameAllocator
    {

        let pages_count = pages.clone().count();
        let frames_count = frames.clone().count();
        if pages_count != frames_count {
            error!("map_to_internal(): page count {} must equal frame count {}!", pages_count, frames_count);
            return Err("map_to_internal(): page count must equal frame count");
        }
            

        // iterate over pages and frames in lockstep
        for (page, frame) in pages.clone().zip(frames) {

            let mut p3 = self.p4_mut().next_table_create(page.p4_index(), flags, allocator);
            let mut p2 = p3.next_table_create(page.p3_index(), flags, allocator);
            let mut p1 = p2.next_table_create(page.p2_index(), flags, allocator);

            if !p1[page.p1_index()].is_unused() {
                error!("map_to() page {:#x} -> frame {:#X}, page was already in use!", page.start_address(), frame.start_address());
                return Err("page was already in use");
            } 

            p1[page.p1_index()].set(frame, flags | EntryFlags::PRESENT);
        }

        Ok(MappedPages {
            page_table_p4: self.target_p4.clone(),
            pages: pages,
            allocated: None,
        })
    }


    /// creates a mapping for a specific page -> specific frame
    pub fn map_to<A>(&mut self, page: Page, frame: Frame, flags: EntryFlags, allocator: &mut A)
        -> Result<MappedPages, &'static str>
        where A: FrameAllocator
    {
        self.internal_map_to(Page::range_inclusive(page, page), Frame::range_inclusive(frame.clone(), frame), flags, allocator)
    }

    /// maps the given Page to a randomly selected (newly allocated) Frame
    pub fn map<A>(&mut self, page: Page, flags: EntryFlags, allocator: &mut A)
        -> Result<MappedPages, &'static str>
        where A: FrameAllocator
    {
        self.internal_map(Page::range_inclusive(page, page), flags, allocator)
    }

    /// maps the given `Page`s to a randomly selected (newly allocated) Frame
    pub fn map_pages<A>(&mut self, pages: PageIter, flags: EntryFlags, allocator: &mut A)
        -> Result<MappedPages, &'static str>
        where A: FrameAllocator
    {
        self.internal_map(pages, flags, allocator)
    }


    /// maps the given contiguous range of Frames `frame_range` to contiguous `Page`s starting at `start_page`
    pub fn map_frames<A>(&mut self, frames: FrameIter, start_page: Page, flags: EntryFlags, allocator: &mut A)
        -> Result<MappedPages, &'static str>
        where A: FrameAllocator
    {
        let iter_size = frames.clone().count() - 1; // -1 because it's inclusive
        self.internal_map_to(Page::range_inclusive(start_page, start_page + iter_size), frames, flags, allocator)
    }


    /// maps the given `AllocatedPages` to the given actual frames.
    /// Consumes the given `AllocatedPages` and returns a `MappedPages` object which contains that `AllocatedPages` object.
    pub fn map_allocated_pages_to<A>(&mut self, allocated_pages: AllocatedPages, frames: FrameIter, flags: EntryFlags, allocator: &mut A)
        -> Result<MappedPages, &'static str>
        where A: FrameAllocator
    {
        let mut ret = self.internal_map_to(allocated_pages.pages.clone(), frames, flags, allocator);
        if let Ok(ref mut r) = ret {
            r.allocated = Some(allocated_pages);
        }
        ret
    }


    /// maps the given `AllocatedPages` to randomly chosen (allocated) frames
    /// Consumes the given `AllocatedPages` and returns a `MappedPages` object which contains that `AllocatedPages` object.
    pub fn map_allocated_pages<A>(&mut self, allocated_pages: AllocatedPages, flags: EntryFlags, allocator: &mut A)
        -> Result<MappedPages, &'static str>
        where A: FrameAllocator
    {
        let mut ret = self.internal_map(allocated_pages.pages.clone(), flags, allocator);
        if let Ok(ref mut r) = ret {
            r.allocated = Some(allocated_pages);
        }
        ret    
    }


    /// Change the permissions (`new_flags`) of the given `MappedPages`'s page table entries
    pub fn remap(&mut self, pages: &MappedPages, new_flags: EntryFlags) -> Result<(), &'static str> {
        use x86_64::instructions::tlb;

        for page in pages.pages.clone() {

            let p1 = try!(self.p4_mut()
                .next_table_mut(page.p4_index())
                .and_then(|p3| p3.next_table_mut(page.p3_index()))
                .and_then(|p2| p2.next_table_mut(page.p2_index()))
                .ok_or("mapping code does not support huge pages")
            );
            
            let frame = try!(p1[page.p1_index()].pointed_frame().ok_or("remap(): page not mapped"));
            p1[page.p1_index()].set(frame, new_flags | EntryFlags::PRESENT);

            tlb::flush(x86_64::VirtualAddress(page.start_address()));
            broadcast_tlb_shootdown(page.start_address());
        }

        Ok(())
    }   


    /// Remove the virtual memory mapping for the given `Page`s.
    fn unmap<A>(&mut self, pages: PageIter, _allocator: &mut A) -> Result<(), &'static str> 
        where A: FrameAllocator
    {
        for page in pages {

            use x86_64::instructions::tlb;

            if self.translate_page(page).is_none() {
                error!("unmap(): page {:?} was not mapped!", page);
                return Err("unmap(): page was not mapped");
            }

            let p1 = try!(self.p4_mut()
                .next_table_mut(page.p4_index())
                .and_then(|p3| p3.next_table_mut(page.p3_index()))
                .and_then(|p2| p2.next_table_mut(page.p2_index()))
                .ok_or("mapping code does not support huge pages")
            );
            
            let _frame = try!(p1[page.p1_index()].pointed_frame().ok_or("unmap(): page not mapped"));
            p1[page.p1_index()].set_unused();

            tlb::flush(x86_64::VirtualAddress(page.start_address()));
            broadcast_tlb_shootdown(page.start_address());
            
            // TODO free p(1,2,3) table if empty
            // allocator.deallocate_frame(frame);
        }

        Ok(())
    }
}



/// broadcasts TLB shootdown IPI
fn broadcast_tlb_shootdown(vaddr: VirtualAddress) {
    
    use interrupts::apic::get_my_apic;
    if let Some(my_lapic) = get_my_apic() {
        // trace!("remap(): (AP {}) sending tlb shootdown ipi for vaddr {:#X}", my_lapic.apic_id, vaddr);
        my_lapic.write().send_tlb_shootdown_ipi(vaddr);
    }
}





/// Represents a mapped range of virtual addresses, specified in pages. 
/// This object also represents ownership of those pages; if this object falls out of scope,
/// it will be dropped, and the pages will be unmapped, and if they were allocated, then also de-allocated. 
/// Thus, it ensures memory safety by guaranteeing that this object must be held 
/// in order to access data stored in these mapped pages, 
/// just like a MutexGuard guarantees that data protected by a Mutex can only be accessed
/// while that Mutex's lock is held. 
#[derive(Debug)]
pub struct MappedPages {
    /// The P4 Frame of the ActivePageTable that this MappedPages was originally mapped into. 
    page_table_p4: Frame,
    /// The actual range of pages contained by this mapping
    pages: PageIter,
    /// The AllocatedPages that were covered by this mapping. 
    /// If Some, it means the pages were allocated by the virtual_address_allocator
    /// and should be deallocated. 
    /// If None, then it was pre-reserved without allocation and doesn't need to be "freed",
    /// but rather just unmapped.
    allocated: Option<AllocatedPages>,
}

impl MappedPages {
	/// Returns the start address of the first page. 
	pub fn start_address(&self) -> VirtualAddress {
		self.pages.start_address()
	}

    /// Constructs a MappedPages object from an already existing mapping.
    /// Useful for creating idle task Stacks, for example. 
    pub fn from_existing(already_mapped_pages: PageIter) -> MappedPages {
        MappedPages {
            page_table_p4: get_current_p4(),
            pages: already_mapped_pages,
            allocated: None,
        }
    }
}

impl Drop for MappedPages {
    #[inline]
    fn drop(&mut self) {
        // skip logging temp page unmapping, since it's the most common
        if self.pages.start != Page::containing_address(TEMPORARY_PAGE_VIRT_ADDR) {
            trace!("MappedPages::drop(): unmapping MappedPages start: {:?} to end: {:?}", self.pages.start, self.pages.end);
        }

        // TODO FIXME: could add "is_kernel" field to MappedPages struct to check whether this is a kernel mapping.
        // TODO FIXME: if it was a kernel mapping, then we don't need to do this P4 value check (it could be unmapped on any page table)

        assert!(get_current_p4() == self.page_table_p4, 
                "MappedPages::drop(): current P4 {:?} must equal original P4 {:?}, \
                 cannot unmap MappedPages from a different page table than they were originally mapped to!",
                 get_current_p4(), self.page_table_p4);

        let mut frame_allocator = match FRAME_ALLOCATOR.try() {
            Some(fa) => fa.lock(),
            _ => {
                error!("MappedPages::drop(): couldn't get FRAME_ALLOCATOR!");
                return;
            }
        };
        
        let mut active_table = ActivePageTable::new(get_current_p4()); // already checked the P4 value
        if let Err(e) = active_table.unmap(self.pages.clone(), frame_allocator.deref_mut()) {
            error!("MappedPages::drop(): failed to unmap, error: {:?}", e);
        }

        // Note that the AllocatedPages will automatically be dropped here too,
        // we do not need to call anything to make that happen
    }
}