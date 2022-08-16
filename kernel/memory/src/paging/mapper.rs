// Copyright 2016 Philipp Oppermann. See the README.md
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use core::{
    mem,
    fmt::{self, Write},
    ops::Deref,
    ptr::Unique,
    slice,
};
use {BROADCAST_TLB_SHOOTDOWN_FUNC, VirtualAddress, PhysicalAddress, Page, Frame, FrameRange, AllocatedPages, AllocatedFrames}; 
use paging::{
    get_current_p4,
    PageRange,
    table::{P4, Table, Level4},
};
use kernel_config::memory::{PAGE_SIZE, ENTRIES_PER_PAGE_TABLE};
use super::{EntryFlags, tlb_flush_virt_addr};
use zerocopy::FromBytes;
use page_table_entry::UnmapResult;

pub struct Mapper {
    p4: Unique<Table<Level4>>,
    /// The Frame contaning the top-level P4 page table.
    pub(crate) target_p4: Frame,
}

impl Mapper {
    pub(crate) fn from_current() -> Mapper {
        Self::with_p4_frame(get_current_p4())
    }

    pub(crate) fn with_p4_frame(p4: Frame) -> Mapper {
        Mapper { 
            p4: Unique::new(P4).unwrap(), // cannot panic because we know the P4 value is valid
            target_p4: p4,
        }
    }

    pub(crate) fn p4(&self) -> &Table<Level4> {
        unsafe { self.p4.as_ref() }
    }

    pub(crate) fn p4_mut(&mut self) -> &mut Table<Level4> {
        unsafe { self.p4.as_mut() }
    }

    /// Dumps all page table entries at all four page table levels for the given `VirtualAddress`, 
    /// and also shows their `EntryFlags`.
    /// 
    /// The page table details are written to the the given `writer`.
    pub fn dump_pte<W: Write>(&self, writer: &mut W, virtual_address: VirtualAddress) -> fmt::Result {
        let page = Page::containing_address(virtual_address);
        let p4  = self.p4();
        let p3  = p4.next_table(page.p4_index());
        let p2  = p3.and_then(|p3| p3.next_table(page.p3_index()));
        let p1  = p2.and_then(|p2| p2.next_table(page.p2_index()));
        write!(
            writer,
            "VirtualAddress: {:#X}:
                P4 entry:        {:#X}   ({:?})
                P3 entry:        {:#X}   ({:?})
                P2 entry:        {:#X}   ({:?})
                P1 entry: (PTE)  {:#X}   ({:?})",
            virtual_address,
            &p4[page.p4_index()].value(),
            &p4[page.p4_index()].flags(),
            p3.map(|p3| &p3[page.p3_index()]).map(|p3_entry| p3_entry.value()).unwrap_or(0x0),
            p3.map(|p3| &p3[page.p3_index()]).map(|p3_entry| p3_entry.flags()),
            p2.map(|p2| &p2[page.p2_index()]).map(|p2_entry| p2_entry.value()).unwrap_or(0x0),
            p2.map(|p2| &p2[page.p2_index()]).map(|p2_entry| p2_entry.flags()),
            p1.map(|p1| &p1[page.p1_index()]).map(|p1_entry| p1_entry.value()).unwrap_or(0x0),
            p1.map(|p1| &p1[page.p1_index()]).map(|p1_entry| p1_entry.flags()),
        )
    }

    /// Translates a `VirtualAddress` to a `PhysicalAddress` by walking the page tables.
    pub fn translate(&self, virtual_address: VirtualAddress) -> Option<PhysicalAddress> {
        // get the frame number of the page containing the given virtual address,
        // and then the corresponding physical address is that page frame number * page size + offset
        self.translate_page(Page::containing_address(virtual_address))
            .map(|frame| frame.start_address() + virtual_address.page_offset())
    }

    /// Translates a virtual memory `Page` to a physical memory `Frame` by walking the page tables.
    pub fn translate_page(&self, page: Page) -> Option<Frame> {
        let p3 = self.p4().next_table(page.p4_index());

        let huge_page = || {
            p3.and_then(|p3| {
                let p3_entry = &p3[page.p3_index()];
                // 1GiB page?
                if let Some(start_frame) = p3_entry.pointed_frame() {
                    if p3_entry.flags().is_huge() {
                        // address must be 1GiB aligned
                        assert!(start_frame.number() % (ENTRIES_PER_PAGE_TABLE * ENTRIES_PER_PAGE_TABLE) == 0);
                        return Some(Frame::containing_address(PhysicalAddress::new_canonical(
                            PAGE_SIZE * (start_frame.number() + page.p2_index() * ENTRIES_PER_PAGE_TABLE + page.p1_index())
                        )));
                    }
                }
                if let Some(p2) = p3.next_table(page.p3_index()) {
                    let p2_entry = &p2[page.p2_index()];
                    // 2MiB page?
                    if let Some(start_frame) = p2_entry.pointed_frame() {
                        if p2_entry.flags().is_huge() {
                            // address must be 2MiB aligned
                            assert!(start_frame.number() % ENTRIES_PER_PAGE_TABLE == 0);
                            return Some(Frame::containing_address(PhysicalAddress::new_canonical(
                                PAGE_SIZE * (start_frame.number() + page.p1_index())
                            )));
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


    /// Maps the given virtual `AllocatedPages` to the given physical `AllocatedFrames`.
    /// 
    /// Consumes the given `AllocatedPages` and returns a `MappedPages` object which contains those `AllocatedPages`.
    pub fn map_allocated_pages_to(&mut self, pages: AllocatedPages, frames: AllocatedFrames, flags: EntryFlags)
        -> Result<MappedPages, &'static str>
    {
        let mut top_level_flags = flags.clone() | EntryFlags::PRESENT;
        // P4, P3, and P2 entries should never set NO_EXECUTE, only the lowest-level P1 entry should. 
        // top_level_flags.set(EntryFlags::WRITABLE, true); // is the same true for the WRITABLE bit?
        top_level_flags.set(EntryFlags::NO_EXECUTE, false);
        // Currently we cannot use the EXCLUSIVE bit for page table frames (P4, P3, P2),
        // because another page table frame may re-use (create another alias for) it without us knowing here.
        // Only the lowest-level P1 entry can be considered exclusive, only if it's mapped truly exclusively using this function.
        top_level_flags.set(EntryFlags::EXCLUSIVE, false);
        let actual_flags = flags | EntryFlags::EXCLUSIVE | EntryFlags::PRESENT;

        let pages_count = pages.size_in_pages();
        let frames_count = frames.size_in_frames();
        if pages_count != frames_count {
            error!("map_allocated_pages_to(): pages {:?} count {} must equal frames {:?} count {}!", 
                pages, pages_count, frames, frames_count
            );
            return Err("map_allocated_pages_to(): page count must equal frame count");
        }

        // iterate over pages and frames in lockstep
        for (page, frame) in pages.deref().clone().into_iter().zip(frames.deref().clone().into_iter()) {
            let p3 = self.p4_mut().next_table_create(page.p4_index(), top_level_flags);
            let p2 = p3.next_table_create(page.p3_index(), top_level_flags);
            let p1 = p2.next_table_create(page.p2_index(), top_level_flags);

            if !p1[page.p1_index()].is_unused() {
                error!("map_allocated_pages_to(): page {:#X} -> frame {:#X}, page was already in use!", page.start_address(), frame.start_address());
                return Err("map_allocated_pages_to(): page was already in use");
            } 

            p1[page.p1_index()].set_entry(frame, actual_flags);
        }

        // Currently we forget the actual AllocatedPages object because
        // there is no easy/efficient way to store a dynamic list of non-contiguous frames (would require Vec).
        // This is okay because we will deallocate each of these frames when this MappedPages object is dropped
        // and each of the page table entries for its pages are cleared.
        core::mem::forget(frames);

        Ok(MappedPages {
            page_table_p4: self.target_p4.clone(),
            pages,
            flags: actual_flags,
        })
    }


    /// Maps the given `AllocatedPages` to randomly chosen (allocated) physical frames.
    /// 
    /// Consumes the given `AllocatedPages` and returns a `MappedPages` object which contains those `AllocatedPages`.
    pub fn map_allocated_pages(&mut self, pages: AllocatedPages, flags: EntryFlags)
        -> Result<MappedPages, &'static str>
    {
        let mut top_level_flags = flags.clone() | EntryFlags::PRESENT;
        // P4, P3, and P2 entries should never set NO_EXECUTE, only the lowest-level P1 entry should. 
        // top_level_flags.set(EntryFlags::WRITABLE, true); // is the same true for the WRITABLE bit?
        top_level_flags.set(EntryFlags::NO_EXECUTE, false);
        // Currently we cannot use the EXCLUSIVE bit for page table frames (P4, P3, P2),
        // because another page table frame may re-use (create another alias for) it without us knowing here.
        // Only the lowest-level P1 entry can be considered exclusive, only if it's mapped truly exclusively using this function.
        top_level_flags.set(EntryFlags::EXCLUSIVE, false);
        let actual_flags = flags | EntryFlags::EXCLUSIVE | EntryFlags::PRESENT;

        for page in pages.deref().clone() {
            let af = frame_allocator::allocate_frames(1).ok_or("map_allocated_pages(): couldn't allocate new frame, out of memory")?;
            let frame = *af.start();
            core::mem::forget(af); // we currently forget frames allocated as page table frames since we don't yet have a way to track them.

            let p3 = self.p4_mut().next_table_create(page.p4_index(), top_level_flags);
            let p2 = p3.next_table_create(page.p3_index(), top_level_flags);
            let p1 = p2.next_table_create(page.p2_index(), top_level_flags);

            if !p1[page.p1_index()].is_unused() {
                error!("map_allocated_pages(): page {:#X} -> frame {:#X}, page was already in use!",
                    page.start_address(), frame.start_address()
                );
                return Err("map_allocated_pages(): page was already in use");
            } 

            p1[page.p1_index()].set_entry(frame, actual_flags);
        }

        Ok(MappedPages {
            page_table_p4: self.target_p4.clone(),
            pages,
            flags: actual_flags,
        })
    }
}

// This implementation block contains a hacky function for non-bijective mappings 
// that shouldn't be exposed to most other OS components, especially applications.
impl Mapper {
    /// An unsafe escape hatch that allows one to map the given virtual `AllocatedPages` 
    /// to the given range of physical `frames`. 
    ///
    /// This is unsafe because it violates Theseus's bijective mapping guarantee, 
    /// in which only one virtual page can map to a given physical frame,
    /// which preserves Rust's knowledge of language-level aliasing and thus its safety checks.
    ///
    /// As such, the pages mapped here will be marked as non-`EXCLUSIVE`, regardless of the `flags` passed in.
    /// 
    /// Consumes the given `AllocatedPages` and returns a `MappedPages` object which contains those `AllocatedPages`.
    #[doc(hidden)]
    pub unsafe fn map_to_non_exclusive(mapper: &mut Self, pages: AllocatedPages, frames: FrameRange, flags: EntryFlags)
        -> Result<MappedPages, &'static str>
    {
        let mut top_level_flags = flags.clone() | EntryFlags::PRESENT;
        // P4, P3, and P2 entries should never set NO_EXECUTE, only the lowest-level P1 entry should. 
        // top_level_flags.set(EntryFlags::WRITABLE, true); // is the same true for the WRITABLE bit?
        top_level_flags.set(EntryFlags::NO_EXECUTE, false);
        // Currently we cannot use the EXCLUSIVE bit for page table frames (P4, P3, P2),
        // because another page table frame may re-use (create another alias for) it without us knowing here.
        top_level_flags.set(EntryFlags::EXCLUSIVE, false);
        // In fact, in this function, none of the frames can be mapped as exclusive
        // because we're not accepting the `AllocatedFrames` type. 
        let mut actual_flags = flags | EntryFlags::PRESENT;
        actual_flags.set(EntryFlags::EXCLUSIVE, false);
        

        let pages_count = pages.size_in_pages();
        let frames_count = frames.size_in_frames();
        if pages_count != frames_count {
            error!("map_to_non_exclusive(): pages {:?} count {} must equal frames {:?} count {}!", 
                pages, pages_count, frames, frames_count
            );
            return Err("map_to_non_exclusive(): page count must equal frame count");
        }

        // iterate over pages and frames in lockstep
        for (page, frame) in pages.deref().clone().into_iter().zip(frames.deref().clone().into_iter()) {
            let p3 = mapper.p4_mut().next_table_create(page.p4_index(), top_level_flags);
            let p2 = p3.next_table_create(page.p3_index(), top_level_flags);
            let p1 = p2.next_table_create(page.p2_index(), top_level_flags);

            if !p1[page.p1_index()].is_unused() {
                error!("map_to_non_exclusive(): page {:#X} -> frame {:#X}, page was already in use!", page.start_address(), frame.start_address());
                return Err("map_to_non_exclusive(): page was already in use");
            } 

            p1[page.p1_index()].set_entry(frame, actual_flags);
        }

        Ok(MappedPages {
            page_table_p4: mapper.target_p4.clone(),
            pages,
            flags: actual_flags,
        })
    }
}


/// Represents a contiguous range of virtual memory pages that are currently mapped. 
/// A `MappedPages` object can only have a single range of contiguous pages, not multiple disjoint ranges.
/// This does not guarantee that its pages are mapped to frames that are contiguous in physical memory.
/// 
/// This object also represents ownership of those pages; if this object falls out of scope,
/// it will be dropped, and the pages will be unmapped and then also de-allocated. 
/// Thus, it ensures memory safety by guaranteeing that this object must be held 
/// in order to access data stored in these mapped pages, much like a guard type.
#[derive(Debug)]
pub struct MappedPages {
    /// The Frame containing the top-level P4 page table that this MappedPages was originally mapped into. 
    page_table_p4: Frame,
    /// The range of allocated virtual pages contained by this mapping.
    pages: AllocatedPages,
    // The EntryFlags that define the page permissions of this mapping
    flags: EntryFlags,
}
impl Deref for MappedPages {
    type Target = PageRange;
    fn deref(&self) -> &PageRange {
        self.pages.deref()
    }
}

impl MappedPages {
    /// Returns an empty MappedPages object that performs no allocation or mapping actions. 
    /// Can be used as a placeholder, but will not permit any real usage. 
    pub const fn empty() -> MappedPages {
        MappedPages {
            page_table_p4: Frame::containing_address(PhysicalAddress::zero()),
            pages: AllocatedPages::empty(),
            flags: EntryFlags::zero(),
        }
    }

    /// Returns the flags that describe this `MappedPages` page table permissions.
    pub fn flags(&self) -> EntryFlags {
        self.flags
    }

    /// Merges the given `MappedPages` object `mp` into this `MappedPages` object (`self`).
    ///
    /// For example, if you have the following `MappedPages` objects:    
    /// * this mapping, with a page range including one page at 0x2000
    /// * `mp`, with a page range including two pages at 0x3000 and 0x4000
    /// Then this `MappedPages` object will be updated to cover three pages from `[0x2000:0x4000]` inclusive.
    /// 
    /// In addition, the `MappedPages` objects must have the same flags and page table root frame
    /// (i.e., they must have all been mapped using the same set of page tables).
    /// 
    /// If an error occurs, such as the `mappings` not being contiguous or having different flags, 
    /// then a tuple including an error message and the original `mp` will be returned,
    /// which prevents the `mp` from being dropped. 
    /// 
    /// # Note
    /// No remapping actions or page reallocations will occur on either a failure or a success.
    pub fn merge(&mut self, mut mp: MappedPages) -> Result<(), (&'static str, MappedPages)> {
        if mp.page_table_p4 != self.page_table_p4 {
            error!("MappedPages::merge(): mappings weren't mapped using the same page table: {:?} vs. {:?}",
                self.page_table_p4, mp.page_table_p4);
            return Err(("failed to merge MappedPages that were mapped into different page tables", mp));
        }
        if mp.flags != self.flags {
            error!("MappedPages::merge(): mappings had different flags: {:?} vs. {:?}",
                self.flags, mp.flags);
            return Err(("failed to merge MappedPages that were mapped with different flags", mp));
        }

        // Attempt to merge the page ranges together, which will fail if they're not contiguous.
        // First, take ownership of the AllocatedPages inside of the `mp` argument.
        let second_alloc_pages_owned = core::mem::replace(&mut mp.pages, AllocatedPages::empty());
        if let Err(orig) = self.pages.merge(second_alloc_pages_owned) {
            // Upon error, restore the `mp.pages` AllocatedPages that we took ownership of.
            mp.pages = orig;
            error!("MappedPages::merge(): mappings not virtually contiguous: first ends at {:?}, second starts at {:?}",
                self.pages.end(), mp.pages.start()
            );
            return Err(("failed to merge MappedPages that weren't virtually contiguous", mp));
        }

        // Ensure the existing mapping doesn't run its drop handler and unmap its pages.
        mem::forget(mp); 
        Ok(())
    }

    /// Splits this `MappedPages` into two separate `MappedPages` objects:
    /// * `[beginning : at_page - 1]`
    /// * `[at_page : end]`
    /// 
    /// This function follows the behavior of [`core::slice::split_at()`],
    /// thus, either one of the returned `MappedPages` objects may be empty. 
    /// * If `at_page == self.pages.start`, the first returned `MappedPages` object will be empty.
    /// * If `at_page == self.pages.end + 1`, the second returned `MappedPages` object will be empty.
    /// 
    /// Returns an `Err` containing this `MappedPages` (`self`) if `at_page` is not within its bounds.
    /// 
    /// # Note
    /// No remapping actions or page reallocations will occur on either a failure or a success.
    /// 
    /// [`core::slice::split_at()`]: https://doc.rust-lang.org/core/primitive.slice.html#method.split_at
    pub fn split(mut self, at_page: Page) -> Result<(MappedPages, MappedPages), MappedPages> {
        // Take ownership of the `AllocatedPages` inside of the `MappedPages` so we can split it.
        let alloc_pages_owned = core::mem::replace(&mut self.pages, AllocatedPages::empty());

        match alloc_pages_owned.split(at_page) {
            Ok((first_ap, second_ap)) => Ok((
                MappedPages {
                    page_table_p4: self.page_table_p4,
                    pages: first_ap,
                    flags: self.flags,
                },
                MappedPages {
                    page_table_p4: self.page_table_p4,
                    pages: second_ap,
                    flags: self.flags,
                }
                // When returning here, `self` will be dropped, but it's empty so it has no effect.
            )),
            Err(orig_ap) => {
                // Upon error, restore the `self.pages` (`AllocatedPages`) that we took ownership of.
                self.pages = orig_ap;
                Err(self)
            }
        }
    }

    
    /// Creates a deep copy of this `MappedPages` memory region,
    /// by duplicating not only the virtual memory mapping
    /// but also the underlying physical memory frames. 
    /// 
    /// The caller can optionally specify new flags for the duplicated mapping,
    /// otherwise, the same flags as the existing `MappedPages` will be used. 
    /// This is useful for when you want to modify contents in the new pages,
    /// since it avoids extra `remap()` operations.
    /// 
    /// Returns a new `MappedPages` object with the same in-memory contents
    /// as this object, but at a completely new memory region.
    pub fn deep_copy(&self, new_flags: Option<EntryFlags>, active_table_mapper: &mut Mapper) -> Result<MappedPages, &'static str> {
        warn!("MappedPages::deep_copy() has not been adequately tested yet.");
        let size_in_pages = self.size_in_pages();

        use paging::allocate_pages;
        let new_pages = allocate_pages(size_in_pages).ok_or_else(|| "Couldn't allocate_pages()")?;

        // we must temporarily map the new pages as Writable, since we're about to copy data into them
        let new_flags = new_flags.unwrap_or(self.flags);
        let needs_remapping = !new_flags.is_writable(); 
        let mut new_mapped_pages = active_table_mapper.map_allocated_pages(
            new_pages, 
            new_flags | EntryFlags::WRITABLE, // force writable
        )?;

        // perform the actual copy of in-memory content
        // TODO: there is probably a better way to do this, e.g., `rep stosq/movsq` or something
        {
            type PageContent = [u8; PAGE_SIZE];
            let source: &[PageContent] = self.as_slice(0, size_in_pages)?;
            let dest: &mut [PageContent] = new_mapped_pages.as_slice_mut(0, size_in_pages)?;
            dest.copy_from_slice(source);
        }

        if needs_remapping {
            new_mapped_pages.remap(active_table_mapper, new_flags)?;
        }
        
        Ok(new_mapped_pages)
    }

    
    /// Change the permissions (`new_flags`) of this `MappedPages`'s page table entries.
    ///
    /// Note that attempting to change certain "reserved" flags will have no effect. 
    /// For example, arbitrarily setting the `EXCLUSIVE` bit would cause unsafety, so it cannot be changed.
    pub fn remap(&mut self, active_table_mapper: &mut Mapper, new_flags: EntryFlags) -> Result<(), &'static str> {
        if self.size_in_pages() == 0 { return Ok(()); }

        // Use the existing value of the `EXCLUSIVE` flag rather than whatever value was passed in.
        let mut new_flags = new_flags;
        new_flags.set(EntryFlags::EXCLUSIVE, self.flags.is_exclusive());

        if new_flags == self.flags {
            trace!("remap(): new_flags were the same as existing flags, doing nothing.");
            return Ok(());
        }

        for page in self.pages.clone() {
            let p1 = active_table_mapper.p4_mut()
                .next_table_mut(page.p4_index())
                .and_then(|p3| p3.next_table_mut(page.p3_index()))
                .and_then(|p2| p2.next_table_mut(page.p2_index()))
                .ok_or("mapping code does not support huge pages")?;
            
            let frame = p1[page.p1_index()].pointed_frame().ok_or("remap(): page not mapped")?;
            p1[page.p1_index()].set_entry(frame, new_flags | EntryFlags::PRESENT);

            tlb_flush_virt_addr(page.start_address());
        }
        
        if let Some(func) = BROADCAST_TLB_SHOOTDOWN_FUNC.get() {
            func(self.pages.deref().clone());
        }

        self.flags = new_flags;
        Ok(())
    }   
    
    /// Consumes and unmaps this `MappedPages` object without auto-deallocating its `AllocatedPages` and `AllocatedFrames`,
    /// allowing the caller to continue using them directly, e.g., reusing them for a future mapping. 
    /// This removes the need to attempt to to reallocate those same pages or frames on a separate code path.
    ///
    /// Note that only the first contiguous range of `AllocatedFrames` will be returned, if any were unmapped.
    /// All other non-contiguous ranges will be auto-dropped and deallocated.
    /// This is due to how frame deallocation works.
    pub fn unmap_into_parts(mut self, active_table_mapper: &mut Mapper) -> Result<(AllocatedPages, Option<AllocatedFrames>), Self> {
        match self.unmap(active_table_mapper) {
            Ok(first_frames) => {
                let pages = mem::replace(&mut self.pages, AllocatedPages::empty());
                Ok((pages, first_frames))
            }
            Err(e) => {
                error!("MappedPages::unmap_into_parts(): failed to unmap {:?}, error: {}", self, e);
                return Err(self);
            }
        }
    }


    /// Remove the virtual memory mapping represented by this `MappedPages`.
    ///
    /// This must NOT be public because it does not take ownership of this `MappedPages` object (`self`).
    /// This is to allow it to be invoked from the `MappedPages` drop handler.
    ///
    /// Returns the **first, contiguous** range of frames that was mapped to these pages.
    /// If there are multiple discontiguous ranges of frames that were unmapped, 
    /// or the frames were not mapped bijectively (i.e., multiple pages mapped to these frames),
    /// then only the first contiguous range of frames will be returned.
    ///
    /// TODO: a few optional improvements could be made here:
    ///   (1) Accept an `Option<&mut Vec<AllocatedFrames>>` argument that allows the caller to 
    ///       recover **all** `AllocatedFrames` unmapped during this function, not just the first contiguous frame range.
    ///   (2) Redesign this to take/consume `self` by ownership, and expose it as the only unmap function,
    ///       avoiding the need for a separate `unmap_into_parts()` function. 
    ///       We could then use `mem::replace(&mut self, MappedPages::empty())` in the drop handler 
    ///       to obtain ownership of `self`, which would allow us to transfer ownership of the dropped `MappedPages` here.
    ///
    fn unmap(&mut self, active_table_mapper: &mut Mapper) -> Result<Option<AllocatedFrames>, &'static str> {
        if self.size_in_pages() == 0 { return Ok(None); }

        if active_table_mapper.target_p4 != self.page_table_p4 {
            error!("BUG: MappedPages::unmap(): {:?}\n    current P4 {:?} must equal original P4 {:?}, \
                cannot unmap MappedPages from a different page table than they were originally mapped to!",
                self, get_current_p4(), self.page_table_p4
            );
            return Err(
                "BUG: MappedPages::unmap(): current P4 must equal original P4, \
                cannot unmap MappedPages from a different page table than they were originally mapped to!"
            );
        }   

        let mut first_frame_range: Option<AllocatedFrames> = None; // this is what we'll return
        let mut current_frame_range: Option<AllocatedFrames> = None;

        for page in self.pages.clone() {            
            let p1 = active_table_mapper.p4_mut()
                .next_table_mut(page.p4_index())
                .and_then(|p3| p3.next_table_mut(page.p3_index()))
                .and_then(|p2| p2.next_table_mut(page.p2_index()))
                .ok_or("mapping code does not support huge pages")?;
            let pte = &mut p1[page.p1_index()];
            if pte.is_unused() {
                return Err("unmap(): page not mapped");
            }

            let unmapped_frames = pte.set_unmapped();
            tlb_flush_virt_addr(page.start_address());

            // Here, create (or extend) a contiguous ranges of frames here based on the `unmapped_frames`
            // freed from the newly-unmapped P1 PTE entry above.
            match unmapped_frames {
                UnmapResult::Exclusive(newly_unmapped_frames) => {
                    let newly_unmapped_frames: AllocatedFrames = newly_unmapped_frames.into();
                    if let Some(mut curr_frames) = current_frame_range.take() {
                        match curr_frames.merge(newly_unmapped_frames) {
                            Ok(()) => {
                                // Here, the newly unmapped frames were contiguous with the current frame_range,
                                // and we successfully merged them into a single range of AllocatedFrames.
                                current_frame_range = Some(curr_frames);
                            }
                            Err(newly_unmapped_frames) => {
                                // Here, the newly unmapped frames were **NOT** contiguous with the current_frame_range,
                                // so we "finish" the current_frame_range (it's already been "taken") and start a new one
                                // based on the newly unmapped frames.
                                current_frame_range = Some(newly_unmapped_frames);
                                
                                // If this is the first frame range we've unmapped, don't drop it -- save it as the return value.
                                if first_frame_range.is_none() {
                                    first_frame_range = Some(curr_frames);
                                } else {
                                    // If this is NOT the first frame range we've unmapped, then go ahead and drop it now,
                                    // otherwise there will not be any other opportunity for it to be dropped.
                                    //
                                    // TODO: here in the future, we could add it to the optional input list (see this function's doc comments)
                                    //       of AllocatedFrames to return, i.e., `Option<&mut Vec<AllocatedFrames>>`.
                                    trace!("MappedPages::unmap(): dropping additional non-contiguous frames {:?}", curr_frames);
                                    // curr_frames is dropped here
                                }
                            }
                        }
                    } else {
                        // This was the first frames we unmapped, so start a new current_frame_range.
                        current_frame_range = Some(newly_unmapped_frames);
                    }
                }
                UnmapResult::NonExclusive(_frames) => {
                    // trace!("Note: FYI: page {:X?} -> frames {:X?} was just unmapped but not mapped as EXCLUSIVE.", page, _frames);
                }
            }
        }
    
        #[cfg(not(bm_map))]
        {
            if let Some(func) = BROADCAST_TLB_SHOOTDOWN_FUNC.get() {
                func(self.pages.deref().clone());
            }
        }

        // Ensure that we return at least some frame range, even if we broke out of the above loop early.
        Ok(first_frame_range.or(current_frame_range))
    }


    /// Reinterprets this `MappedPages`'s underlying memory region as a struct of the given type `T`,
    /// i.e., overlays a struct on top of this mapped memory region. 
    /// 
    /// # Requirements
    /// The type `T` must implement the `FromBytes` trait, which is similar to the requirements 
    /// of a "plain old data" type, in that it cannot contain Rust references (`&` or `&mut`).
    /// This makes sense because there is no valid way to reinterpret a region of untyped memory 
    /// as a Rust reference. 
    /// In addition, if we did permit that, a Rust reference created from unchecked memory contents
    /// could never be valid, safe, or sound, as it could allow random memory access 
    /// (just like with an arbitrary pointer dereference) that could break isolation.
    /// 
    /// To satisfy this condition, you can use `#[derive(FromBytes)]` on your struct type `T`,
    /// which will only compile correctly if the struct can be validly constructed 
    /// from "untyped" memory, i.e., an array of bytes.
    /// 
    /// # Arguments
    /// `offset`: the offset into the memory region at which the struct is located (where it should start).
    /// 
    /// Returns a reference to the new struct (`&T`) that is formed from the underlying memory region,
    /// with a lifetime dependent upon the lifetime of this `MappedPages` object.
    /// This ensures safety by guaranteeing that the returned struct reference 
    /// cannot be used after this `MappedPages` object is dropped and unmapped.
    pub fn as_type<T: FromBytes>(&self, offset: usize) -> Result<&T, &'static str> {
        let size = mem::size_of::<T>();
        if false {
            debug!("MappedPages::as_type(): requested type {} with size {} at offset {}, MappedPages size {}!",
                core::any::type_name::<T>(),
                size, offset, self.size_in_bytes()
            );
        }

        // check that size of the type T fits within the size of the mapping
        let end = offset + size;
        if end > self.size_in_bytes() {
            error!("MappedPages::as_type(): requested type {} with size {} at offset {}, which is too large for MappedPages of size {}!",
                core::any::type_name::<T>(),
                size, offset, self.size_in_bytes()
            );
            return Err("requested type and offset would not fit within the MappedPages bounds");
        }

        // SAFE: we guarantee the size and lifetime are within that of this MappedPages object
        let t: &T = unsafe { 
            &*((self.pages.start_address().value() + offset) as *const T)
        };

        Ok(t)
    }


    /// Same as [`as_type()`](#method.as_type), but returns a *mutable* reference to the type `T`.
    /// 
    /// Thus, it checks to make sure that the underlying mapping is writable.
    pub fn as_type_mut<T: FromBytes>(&mut self, offset: usize) -> Result<&mut T, &'static str> {
        let size = mem::size_of::<T>();
        if false {
            debug!("MappedPages::as_type_mut(): requested type {} with size {} at offset {}, MappedPages size {}!",
                core::any::type_name::<T>(),
                size, offset, self.size_in_bytes()
            );
        }

        // check flags to make sure mutability is allowed (otherwise a page fault would occur on a write)
        if !self.flags.is_writable() {
            error!("MappedPages::as_type_mut(): requested type {} with size {} at offset {}, but MappedPages weren't writable (flags: {:?})",
                core::any::type_name::<T>(),
                size, offset, self.flags
            );
            return Err("as_type_mut(): MappedPages were not writable");
        }
        
        // check that size of type T fits within the size of the mapping
        let end = offset + size;
        if end > self.size_in_bytes() {
            error!("MappedPages::as_type_mut(): requested type {} with size {} at offset {}, which is too large for MappedPages of size {}!",
                core::any::type_name::<T>(),
                size, offset, self.size_in_bytes()
            );
            return Err("requested type and offset would not fit within the MappedPages bounds");
        }

        // SAFE: we guarantee the size and lifetime are within that of this MappedPages object
        let t: &mut T = unsafe {
            &mut *((self.pages.start_address().value() + offset) as *mut T)
        };

        Ok(t)
    }


    /// Reinterprets this `MappedPages`'s underlying memory region as a slice of any type.
    /// 
    /// It has similar type requirements as the [`as_type()`](#method.as_type) method.
    /// 
    /// # Arguments
    /// * `byte_offset`: the offset (in number of bytes) into the memory region at which the slice should start.
    /// * `length`: the length of the slice, i.e., the number of `T` elements in the slice. 
    ///   Thus, the slice will go from `offset` to `offset` + (sizeof(`T`) * `length`).
    /// 
    /// Returns a reference to the new slice that is formed from the underlying memory region,
    /// with a lifetime dependent upon the lifetime of this `MappedPages` object.
    /// This ensures safety by guaranteeing that the returned slice 
    /// cannot be used after this `MappedPages` object is dropped and unmapped.
    pub fn as_slice<T: FromBytes>(&self, byte_offset: usize, length: usize) -> Result<&[T], &'static str> {
        let size_in_bytes = mem::size_of::<T>() * length;
        if false {
            debug!("MappedPages::as_slice(): requested slice of type {} with length {} (total size {}) at byte_offset {}, MappedPages size {}!",
                core::any::type_name::<T>(),
                length, size_in_bytes, byte_offset, self.size_in_bytes()
            );
        }
        
        // check that size of slice fits within the size of the mapping
        let end = byte_offset + (length * mem::size_of::<T>());
        if end > self.size_in_bytes() {
            error!("MappedPages::as_slice(): requested slice of type {} with length {} (total size {}) at byte_offset {}, which is too large for MappedPages of size {}!",
                core::any::type_name::<T>(),
                length, size_in_bytes, byte_offset, self.size_in_bytes()
            );
            return Err("requested slice length and offset would not fit within the MappedPages bounds");
        }

        // SAFE: we guarantee the size and lifetime are within that of this MappedPages object
        let slc: &[T] = unsafe {
            slice::from_raw_parts((self.pages.start_address().value() + byte_offset) as *const T, length)
        };

        Ok(slc)
    }


    /// Same as [`as_slice()`](#method.as_slice), but returns a *mutable* slice. 
    /// 
    /// Thus, it checks to make sure that the underlying mapping is writable.
    pub fn as_slice_mut<T: FromBytes>(&mut self, byte_offset: usize, length: usize) -> Result<&mut [T], &'static str> {
        let size_in_bytes = mem::size_of::<T>() * length;
        if false {
            debug!("MappedPages::as_slice_mut(): requested slice of type {} with length {} (total size {}) at byte_offset {}, MappedPages size {}!",
                core::any::type_name::<T>(), 
                length, size_in_bytes, byte_offset, self.size_in_bytes()
            );
        }
        
        // check flags to make sure mutability is allowed (otherwise a page fault would occur on a write)
        if !self.flags.is_writable() {
            error!("MappedPages::as_slice_mut(): requested mutable slice of type {} with length {} (total size {}) at byte_offset {}, but MappedPages weren't writable (flags: {:?})",
                core::any::type_name::<T>(),
                length, size_in_bytes, byte_offset, self.flags
            );
            return Err("as_slice_mut(): MappedPages were not writable");
        }

        // check that size of slice fits within the size of the mapping
        let end = byte_offset + (length * mem::size_of::<T>());
        if end > self.size_in_bytes() {
            error!("MappedPages::as_slice_mut(): requested mutable slice of type {} with length {} (total size {}) at byte_offset {}, which is too large for MappedPages of size {}!",
                core::any::type_name::<T>(),
                length, size_in_bytes, byte_offset, self.size_in_bytes()
            );
            return Err("requested slice length and offset would not fit within the MappedPages bounds");
        }

        // SAFE: we guarantee the size and lifetime are within that of this MappedPages object
        let slc: &mut [T] = unsafe {
            slice::from_raw_parts_mut((self.pages.start_address().value() + byte_offset) as *mut T, length)
        };

        Ok(slc)
    }
}

impl Drop for MappedPages {
    fn drop(&mut self) {
        // if self.size_in_pages() > 0 {
        //     trace!("MappedPages::drop(): unmapped MappedPages {:?}, flags: {:?}", &*self.pages, self.flags);
        // }
        
        let mut mapper = Mapper::from_current();
        if let Err(e) = self.unmap(&mut mapper) {
            error!("MappedPages::drop(): failed to unmap, error: {:?}", e);
        }

        // Note that the AllocatedPages will automatically be dropped here too,
        // we do not need to call anything to make that happen.
    }
}


// Exposing private functions to the spillful mapper for benchmarking purposes.
#[cfg(mapper_spillful)]
pub fn mapped_pages_unmap(
    mapped_pages: &mut MappedPages,
    mapper: &mut Mapper,
) -> Result<(), &'static str> {
    mapped_pages.unmap(mapper)?;
    Ok(())
}

#[cfg(mapper_spillful)]
pub fn mapper_from_current() -> Mapper {
    Mapper::from_current()
}
