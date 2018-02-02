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
use memory::{Frame, FrameAllocator};
use core::ptr::Unique;
use kernel_config::memory::{ENTRIES_PER_PAGE_TABLE, PAGE_SIZE};

pub struct Mapper {
    p4: Unique<Table<Level4>>,
}

impl Mapper {
    pub unsafe fn new() -> Mapper {
        Mapper { p4: Unique::new_unchecked(table::P4) }
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

    /// creates a mapping for a specific page -> specific frame
    pub fn map_to<A>(&mut self, page: Page, frame: Frame, flags: EntryFlags, allocator: &mut A)
        where A: FrameAllocator
    {
        let mut p3 = self.p4_mut().next_table_create(page.p4_index(), flags, allocator);
        let mut p2 = p3.next_table_create(page.p3_index(), flags, allocator);
        let mut p1 = p2.next_table_create(page.p2_index(), flags, allocator);

        assert!(p1[page.p1_index()].is_unused(), "map_to() page {:#x} -> frame {:#X}, page was already in use!", page.start_address(), frame.start_address());
        p1[page.p1_index()].set(frame, flags | EntryFlags::PRESENT);
    }

    /// maps the given Page to a randomly selected (newly allocated) Frame
    pub fn map<A>(&mut self, page: Page, flags: EntryFlags, allocator: &mut A)
        where A: FrameAllocator
    {
        let frame = allocator.allocate_frame().expect("Mapper::map() -- out of memory trying to alloc frame");
        self.map_to(page, frame, flags, allocator)
    }

    /// maps the given contiguous range of Frames `frame_range` to contiguous `Page`s starting at `start_page`
    /// `size_in_bytes` specifies the length in bytes of the mapping. 
    pub fn map_frames<A>(&mut self, frame_range: FrameIter, start_page: Page, flags: EntryFlags, allocator: &mut A)
        where A: FrameAllocator
    {
        for (ctr, frame) in frame_range.enumerate() {
            self.map_to(start_page + ctr, frame, flags, allocator);
        }
    }

    /// SPECIAL USE CASES ONLY! 
    /// Just like `map_frames()`, this function maps the given contiguous range of Frames `frame_range` to contiguous `Page`s starting at `start_page`
    /// `size_in_bytes` specifies the length in bytes of the mapping. 
    /// If any pages in the range of requested mappings are already mapped, those are silently skipped. 
    /// Use case:  filling in holes in a range of frames in which some have already been mapped.
    pub fn map_frames_skip_used<A>(&mut self, frame_range: FrameIter, start_page: Page, flags: EntryFlags, allocator: &mut A)
        where A: FrameAllocator
    {
        for (ctr, frame) in frame_range.enumerate() {
            let page = start_page + ctr;
            if self.translate_page(page).is_some() {
                continue;
            }
            self.map_to(page, frame, flags, allocator);
        }
    }

    /// maps the Page containing the given virtual address to the given Frame
    pub fn map_virtual_address<A>(&mut self, virt_addr: VirtualAddress, frame: Frame, flags: EntryFlags, allocator: &mut A)
        where A: FrameAllocator
    {
        let page: Page = Page::containing_address(virt_addr);
        self.map_to(page, frame, flags, allocator)
    }

    /// maps the given frame's physical address to the same virtual address
    pub fn identity_map<A>(&mut self, frame: Frame, flags: EntryFlags, allocator: &mut A)
        where A: FrameAllocator
    {
        let page = Page::containing_address(frame.start_address());
        self.map_to(page, frame, flags, allocator)
    }


    pub fn remap(&mut self, page: Page, new_flags: EntryFlags) {
        use x86_64::instructions::tlb;

        let p1 = self.p4_mut()
            .next_table_mut(page.p4_index())
            .and_then(|p3| p3.next_table_mut(page.p3_index()))
            .and_then(|p2| p2.next_table_mut(page.p2_index()))
            .expect("mapping code does not support huge pages");
        let frame = p1[page.p1_index()].pointed_frame().expect("remap(): page frame not mapped");
        p1[page.p1_index()].set(frame, new_flags | EntryFlags::PRESENT);

        tlb::flush(x86_64::VirtualAddress(page.start_address()));
        broadcast_tlb_shootdown(page.start_address());
    }   


    /// remaps the range of pages specified by the given `PageIter`.
    pub fn remap_pages(&mut self, page_range: PageIter, new_flags: EntryFlags) {
        for page in page_range {
            self.remap(page, new_flags);
        }
    }

    /// Remove the virtual memory mapping for the given `Page`.
    pub fn unmap<A>(&mut self, page: Page, _allocator: &mut A)
        where A: FrameAllocator
    {
        use x86_64::instructions::tlb;

        assert!(self.translate(page.start_address()).is_some());

        let p1 = self.p4_mut()
            .next_table_mut(page.p4_index())
            .and_then(|p3| p3.next_table_mut(page.p3_index()))
            .and_then(|p2| p2.next_table_mut(page.p2_index()))
            .expect("mapping code does not support huge pages");
        let frame = p1[page.p1_index()].pointed_frame().unwrap();
        p1[page.p1_index()].set_unused();
        tlb::flush(x86_64::VirtualAddress(page.start_address()));
        broadcast_tlb_shootdown(page.start_address());
        
        // TODO free p(1,2,3) table if empty
        // allocator.deallocate_frame(frame);
    }

    pub fn unmap_pages<A>(&mut self, page_range: PageIter, allocator: &mut A)
        where A: FrameAllocator
    {
        for page in page_range {
            self.unmap(page, allocator);
        }
    }
}



/// broadcasts TLB shootdown IPI
fn broadcast_tlb_shootdown(vaddr: VirtualAddress) {
    use interrupts::apic::{get_my_apic_id, get_lapics};
    if let Some(ref mut my_lapic) = get_my_apic_id().and_then(|id| get_lapics().get_mut(id)) {
        // trace!("remap(): (AP {}) sending tlb shootdown ipi for vaddr {:#X}", my_lapic.apic_id, vaddr);
        my_lapic.send_tlb_shootdown_ipi(vaddr);
    }
}
