// Copyright 2016 Philipp Oppermann. See the README.md
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use core::ptr::Unique;
use core::ops::DerefMut;
use core::mem;
use core::slice;
use x86_64;
use {BROADCAST_TLB_SHOOTDOWN_FUNC, VirtualAddress, PhysicalAddress, FRAME_ALLOCATOR, FrameIter, Page, Frame, FrameAllocator, AllocatedPages}; 
use paging::{PageIter, get_current_p4, ActivePageTable};
use paging::entry::EntryFlags;
use paging::table::{P4, Table, Level4};
use kernel_config::memory::{ENTRIES_PER_PAGE_TABLE, PAGE_SIZE, TEMPORARY_PAGE_VIRT_ADDR};

pub struct Mapper {
    p4: Unique<Table<Level4>>,
    pub target_p4: Frame,
}

impl Mapper {
    pub unsafe fn new() -> Mapper {
        Mapper { 
            p4: Unique::new_unchecked(P4),
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
            flags: flags,
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
            flags: flags,
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

        let broadcast_tlb_shootdown = try!(
            BROADCAST_TLB_SHOOTDOWN_FUNC.try()
            .ok_or("remap(): TLB shootdown function callback wasn't set yet! Call memory::init() first!")
        );

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
        let broadcast_tlb_shootdown = try!(
            BROADCAST_TLB_SHOOTDOWN_FUNC.try()
            .ok_or("remap(): TLB shootdown function callback wasn't set yet! Call memory::init() first!")
        );

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





/// Represents a mapped range of virtual addresses, specified in pages.
/// The underlying pages that are mapped are guaranteed to be continguous in virtual memory (not physical),
/// and one `MappedPages` object can only have a single range of contiguous pages, not multiple disjoint ranges.
/// 
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
    // The EntryFlags that define the page permissions of this mapping
    flags: EntryFlags,
}

impl MappedPages {
	/// Returns the start address of the first page. 
	pub fn start_address(&self) -> VirtualAddress {
		self.pages.start_address()
	}

    /// Returns the size of this mapping in number of [`Page`]s.
    pub fn size_in_pages(&self) -> usize {
        // add 1 because it's an inclusive range
        self.pages.end.number - self.pages.start.number + 1 
    }

    /// Returns the size of this mapping in number of bytes.
    pub fn size_in_bytes(&self) -> usize {
        self.size_in_pages() * PAGE_SIZE
    }


    /// Constructs a MappedPages object from an already existing mapping.
    /// Useful for creating idle task Stacks, for example. 
    /// TODO FIXME: remove this ability, it's dangerous!!
    pub fn from_existing(already_mapped_pages: PageIter, flags: EntryFlags) -> MappedPages {
        MappedPages {
            page_table_p4: get_current_p4(),
            pages: already_mapped_pages,
            allocated: None,
            flags: flags,
        }
    }


    /// Reinterprets this `MappedPages`'s underyling memory region as a struct of the given type,
    /// i.e., overlays a struct on top of this mapped memory region. 
    /// 
    /// # Arguments
    /// `offset`: the offset into the memory region at which the struct is located (where it should start).
    /// 
    /// Returns a reference to the new struct (`&T`) that is formed from the underlying memory region,
    /// with a lifetime dependent upon the lifetime of this `MappedPages` object.
    /// This ensures safety by guaranteeing that the returned struct reference 
    /// cannot be used after this `MappedPages` object is dropped and unmapped.
    pub fn as_type<T>(&self, offset: usize) -> Result<&T, &'static str> {
        let size = mem::size_of::<T>();
        debug!("MappedPages::as_type(): requested type {} with size {} at offset {}, MappedPages size {}!",
                // SAFE: just for debugging
                unsafe { ::core::intrinsics::type_name::<T>() }, 
                size, offset, self.size_in_bytes()
        );

        // check that size of the type T fits within the size of the mapping
        let end = offset + size;
        if end > self.size_in_bytes() {
            error!("MappedPages::as_type_mut(): requested type {} with size {} at offset {}, which is too large for MappedPages of size {}!",
                    // SAFE: just for debugging
                    unsafe { ::core::intrinsics::type_name::<T>() }, 
                    size, offset, self.size_in_bytes()
            );
            return Err("requested type and offset would not fit within the MappedPages bounds");
        }

        // SAFE: we guarantee the size and lifetime are within that of this MappedPages object
        let t: &T = unsafe {
            mem::transmute(self.pages.start_address() + offset)
        };

        Ok(t)
    }


    /// Same as [`as_type()`](#method.as_type), but returns a *mutable* reference to the type `T`.
    /// 
    /// Thus, it checks to make sure that the underlying mapping is writable.
    pub fn as_type_mut<T>(&self, offset: usize) -> Result<&mut T, &'static str> {
        let size = mem::size_of::<T>();
        debug!("MappedPages::as_type_mut(): requested type {} with size {} at offset {}, MappedPages size {}!",
                // SAFE: just for debugging
                unsafe { ::core::intrinsics::type_name::<T>() }, 
                size, offset, self.size_in_bytes()
        );

        // check flags to make sure mutability is allowed (otherwise a page fault would occur on a write)
        if !self.flags.is_writable() {
            error!("MappedPages::as_type_mut(): requested type {} with size {} at offset {}, but MappedPages weren't writable (flags: {:?})",
                    // SAFE: just for debugging
                    unsafe { ::core::intrinsics::type_name::<T>() }, 
                    size, offset, self.flags
            );
            return Err("as_type_mut(): MappedPages were not writable");
        }
        
        // check that size of type T fits within the size of the mapping
        let end = offset + size;
        if end > self.size_in_bytes() {
            error!("MappedPages::as_type_mut(): requested type {} with size {} at offset {}, which is too large for MappedPages of size {}!",
                    // SAFE: just for debugging
                    unsafe { ::core::intrinsics::type_name::<T>() }, 
                    size, offset, self.size_in_bytes()
            );
            return Err("requested type and offset would not fit within the MappedPages bounds");
        }

        // SAFE: we guarantee the size and lifetime are within that of this MappedPages object
        let t: &mut T = unsafe {
            mem::transmute(self.pages.start_address() + offset)
        };

        Ok(t)
    }


    /// Reinterprets this `MappedPages`'s underyling memory region as a slice of any type.
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
    /// 
    /// Note: it'd be great to merge this into [`as_type()`][#method.as_type], but I cannot figure out how to 
    ///       specify the length of a slice in its static type definition notation, e.g., `&[u8; size]`.
    pub fn as_slice<T>(&self, byte_offset: usize, length: usize) -> Result<&[T], &'static str> {
        let size_in_bytes = mem::size_of::<T>() * length;
        debug!("MappedPages::as_slice(): requested slice of type {} with length {} (total size {}) at byte_offset {}, MappedPages size {}!",
                // SAFE: just for debugging
                unsafe { ::core::intrinsics::type_name::<T>() }, 
                length, size_in_bytes, byte_offset, self.size_in_bytes()
        );
        
        // check that size of slice fits within the size of the mapping
        let end = byte_offset + (length * mem::size_of::<T>());
        if end > self.size_in_bytes() {
            error!("MappedPages::as_slice(): requested slice of type {} with length {} (total size {}) at byte_offset {}, which is too large for MappedPages of size {}!",
                    // SAFE: just for debugging
                    unsafe { ::core::intrinsics::type_name::<T>() }, 
                    length, size_in_bytes, byte_offset, self.size_in_bytes()
            );
            return Err("requested slice length and offset would not fit within the MappedPages bounds");
        }

        // SAFE: we guarantee the size and lifetime are within that of this MappedPages object
        let slc: &[T] = unsafe {
            slice::from_raw_parts((self.pages.start_address() + byte_offset) as *const T, length)
        };

        Ok(slc)
    }


    /// Same as [`as_slice()`](#method.as_slice), but returns a *mutable* slice. 
    /// 
    /// Thus, it checks to make sure that the underlying mapping is writable.
    pub fn as_slice_mut<T>(&self, byte_offset: usize, length: usize) -> Result<&mut [T], &'static str> {
        let size_in_bytes = mem::size_of::<T>() * length;
        debug!("MappedPages::as_slice_mut(): requested slice of type {} with length {} (total size {}) at byte_offset {}, MappedPages size {}!",
                // SAFE: just for debugging
                unsafe { ::core::intrinsics::type_name::<T>() }, 
                length, size_in_bytes, byte_offset, self.size_in_bytes()
        );
        
        // check flags to make sure mutability is allowed (otherwise a page fault would occur on a write)
        if !self.flags.is_writable() {
            error!("MappedPages::as_slice_mut(): requested mutable slice of type {} with length {} (total size {}) at byte_offset {}, but MappedPages weren't writable (flags: {:?}",
                    // SAFE: just for debugging
                    unsafe { ::core::intrinsics::type_name::<T>() }, 
                    length, size_in_bytes, byte_offset, self.flags
            );
            return Err("as_slice_mut(): MappedPages were not writable");
        }

        // check that size of slice fits within the size of the mapping
        let end = byte_offset + (length * mem::size_of::<T>());
        if end > self.size_in_bytes() {
            error!("MappedPages::as_slice_mut(): requested mutable slice of type {} with length {} (total size {}) at byte_offset {}, which is too large for MappedPages of size {}!",
                    // SAFE: just for debugging
                    unsafe { ::core::intrinsics::type_name::<T>() }, 
                    length, size_in_bytes, byte_offset, self.size_in_bytes()
            );
            return Err("requested slice length and offset would not fit within the MappedPages bounds");
        }

        // SAFE: we guarantee the size and lifetime are within that of this MappedPages object
        let slc: &mut [T] = unsafe {
            slice::from_raw_parts_mut((self.pages.start_address() + byte_offset) as *mut T, length)
        };

        Ok(slc)
    }
}


impl Drop for MappedPages {
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