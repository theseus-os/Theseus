//! Provides an allocator for physical memory frames.
//! The minimum unit of allocation is a single frame. 
//!
//! This is currently a copy of the `page_allocator` crate.
//! TODO: extract the common code and create a generic allocator that can be specialized to allocate pages or frames.
//! 
//! This also supports early allocation of frames (up to 32 individual chunks)
//! before heap allocation is available, and does so behind the scenes using the same single interface. 
//! 
//! Once heap allocation is available, it uses a dynamically-allocated list of frame chunks to track allocations.
//! 
//! The core allocation function is [`allocate_frames_deferred()`](fn.allocate_frames_deferred.html), 
//! but there are several convenience functions that offer simpler interfaces for general usage. 
//!
//! # Notes and Missing Features
//! This allocator currently does **not** merge freed chunks (de-fragmentation). 
//! We don't need to do so until we actually run out of address space or until 
//! a requested address is in a chunk that needs to be merged;
//! that's where we should add those merging features in whenever we do so.

#![no_std]
#![feature(const_fn, const_in_array_repeat_expressions)]

extern crate alloc;
#[macro_use] extern crate log;
extern crate kernel_config;
extern crate memory_structs;
extern crate spin;
#[macro_use] extern crate static_assertions;
extern crate intrusive_collections;
use intrusive_collections::Bound;


mod static_array_rb_tree;
// mod static_array_linked_list;


use core::{borrow::Borrow, cmp::{Ordering, min, max}, fmt, ops::{Deref, DerefMut}};
use kernel_config::memory::*;
use memory_structs::{PhysicalAddress, Frame, FrameRange};
use spin::Mutex;
use static_array_rb_tree::*;

const FRAME_SIZE: usize = PAGE_SIZE;
const MIN_FRAME: Frame = Frame::containing_address(PhysicalAddress::zero());
const MAX_FRAME: Frame = Frame::containing_address(PhysicalAddress::new_canonical(usize::MAX));

// /// Regions that are pre-designated for special usage, specifically the kernel's initial identity mapping.
// /// They will be allocated from if an address within them is specifically requested;
// /// otherwise, they will only be allocated from as a "last resort" once all other non-designated address ranges are exhausted.
// ///
// /// Any physical addresses **less than or equal** to this address are considered "designated".
// /// This lower part of the address range covers from 0x0 to the end of the kernel physical address.
// /// 
// // TODO: replace this with the dynamically-discovered end of kernel, bootloader info, or bootloader modules, whichever highest.
// const DESIGNATED_FRAMES_LOW_END: Frame = Frame::containing_address(PhysicalAddress::new_canonical(0x150_0000 - 1));
// /// Any physical addresses **greater than or equal to** this address are considered "designated".
// /// Currently there are no designated regions at the higher end of the address space.
// // TODO: replace this  with Chunks that can each be reserved individually. 
// const DESIGNATED_FRAMES_HIGH_START: Frame = Frame::containing_address(PhysicalAddress::new_canonical(0x1FFDF000)); // hard-coded end of free phys mem in QEMU


/// The single, system-wide list of free physical memory frames.
/// This must be initialized during the bootstrap process after we use the bootloader to identify the physical memory map.
static FREE_FRAME_LIST: Mutex<StaticArrayRBTree<Chunk>> = Mutex::new(StaticArrayRBTree::empty()); 

/// The single, system-wide list of physical memory frames that have already been allocated and are in use. 
/// This is primarily used to track which regions of physical memory are reserved. 
/// We don't actually need to store 
/// We only need to store the custom chunks that were added here (`MemoryType::Reserved`).
///
/// This is necessary to prevent bugs when a device driver or another entity
/// attempts to add a new custom region of frames. 
/// If that new custom region overlaps or is a duplicate of another region, it must be forbidden. 
/// If if overlaps with a free region, that will be detected when trying to allocate it, 
/// but first we also need to see if that new region overlaps with a region that has already been allocated. 
static ALLOCATED_FRAME_LIST: Mutex<StaticArrayRBTree<Chunk>> = Mutex::new(StaticArrayRBTree::empty()); 


/// Initialize the frame allocator with the given list of available and reserved physical memory regions.
///
/// Any regions in either of the lists may overlap, this is checked for and handled properly.
/// Reserved regions take priority -- if a reserved region partially or fully overlaps any part of a free region,
/// that portion will be considered reserved, not free. 
/// 
/// The iterator (`R`) over reserved physical memory regions must be cloneable, 
/// as this runs before heap allocation is available, and we may need to iterate over it multiple times. 
pub fn init<F, R, P>(
    free_physical_memory_areas: F,
    reserved_physical_memory_areas: R,
) -> Result<(), &'static str> 
    where P: Borrow<PhysicalMemoryRegion>,
          F: IntoIterator<Item = P>,
          R: IntoIterator<Item = P> + Clone,
{
    if FREE_FRAME_LIST.lock().len() != 0 || ALLOCATED_FRAME_LIST.lock().len() != 0 {
        return Err("Frame allocator was already initialized, cannot be initialized twice.");
    }

    let mut free_list: [Option<Chunk>; 32] = [None; 32];
    let mut free_list_idx = 0;
    let mut reserved_list: [Option<Chunk>; 32] = [None; 32];
    let mut reserved_list_idx = 0;

    
    for area in free_physical_memory_areas.into_iter() {
        let area = area.borrow();
        debug!("Frame Allocator: looking to add free physical memory area: {:?}", area);

        check_and_add_free_region(
            &area,
            &mut free_list,
            &mut free_list_idx,
            reserved_physical_memory_areas.clone(),
        );
    }

    // // Ensure that no two chunks overlap.
    // // Currently we resolve this by merging the overlapping chunks,
    // // but only if one chunk fully contains another chunk.
    // // This may be undesirable if we don't want to merge a "reserved" and non-reserved chunk.
    // let mut indices_to_remove: [Option<usize>; 32] = [None; 32];
    // let mut itr_index = 0;
    // for (i, elem_opt) in list[..list_idx].iter().enumerate() {
    //     let next_idx = i + 1;
    //     for (other_opt, j) in list[next_idx..list_idx].iter().zip(next_idx..) {
    //         if let (Some(elem), Some(other)) = (elem_opt, other_opt) {
    //             if elem.contains(other.start()) && elem.contains(other.end()) {
    //                 // Here, the `elem` chunk fully contains the entire `other` chunk, 
    //                 // so we can just delete `other` entirely from the list.
    //                 indices_to_remove[itr_index] = Some(j);
    //                 itr_index += 1;
    //                 continue;
    //             } 
    //             if elem.contains(other.start())  ||
    //                 elem.contains(other.end())   || 
    //                 other.contains(elem.start()) ||
    //                 other.contains(elem.end())
    //             {
    //                 error!("Physical memory areas {:?} and {:?} overlap, this is logically incorrect and currently unsupported", elem, other);
    //                 return Err("Physical memory areas are illegally overlapping.");
    //             }
    //         }
    //     }
    // }

    // // Actually remove the duplicate/overlapping chunks that we found earlier. 
    // for idx in indices_to_remove.iter().flatten() {
    //     trace!("### Removed idx {} from list of frame chunks", idx);
    //     let _removed = list[*idx].take();
    // }

    *FREE_FRAME_LIST.lock() = StaticArrayRBTree::new(free_list);

    // FIXME TODO deduplicate and add reserved list to FREE_FRAME_LIST too!

    Ok(())
}



fn check_and_add_free_region<P, R>(
    area: &FrameRange,
    free_list: &mut [Option<Chunk>; 32],
    free_list_idx: &mut usize,
    reserved_physical_memory_areas: R,
)
    where P: Borrow<PhysicalMemoryRegion>,
          R: IntoIterator<Item = P> + Clone,
{
    // This will be set to the frame that is the start of the current free region. 
    let mut current_start = *area.start();
    // This will be set to the frame that is the end of the current free region. 
    let mut current_end = *area.end();
    trace!("looking at sub-area {:X?} to {:X?}", current_start, current_end);

    'inner:
    for reserved in reserved_physical_memory_areas.clone().into_iter() {
        let reserved = &reserved.borrow().frames;
        trace!("\t Comparing with reserved area {:X?}", reserved);
        if reserved.contains(&current_start) {
            info!("\t\t moving current_start from {:X?} to {:X?}", current_start, *reserved.end() + 1);
            current_start = *reserved.end() + 1;
        }
        if &current_start <= reserved.start() && reserved.start() <= &current_end {
        // if reserved.contains(&current_end) {
            // Advance up to the frame right before this reserved region started.
            info!("\t\t moving current_end from {:X?} to {:X?}", current_end, min(current_end, *reserved.start() - 1));
            current_end = min(current_end, *reserved.start() - 1);
            if area.end() <= reserved.end() {
                // Optimization here: the rest of the current area is reserved,
                // so there's no need to keep iterating over the reserved areas.
                info!("\t !!! skipping the rest of the area");
                break 'inner;
            } else {
                let after = FrameRange::new(*reserved.end() + 1, *area.end());
                warn!("moving on to after {:X?}", after);
                // Here: the current area extends past this current reserved area,
                // so there might be another free area that starts after this reserved area.
                check_and_add_free_region(
                    &after,
                    free_list,
                    free_list_idx,
                    reserved_physical_memory_areas.clone(),
                );
            }
        }
    }

    let new_area = FrameRange::new(current_start, current_end);
    if new_area.size_in_frames() > 0 {
        free_list[*free_list_idx] = Some(Chunk {
            typ:  MemoryRegionType::Free,
            frames: new_area,
        });
        *free_list_idx += 1;
    }
}


/// A region of physical memory.
#[derive(Clone, Debug)]
pub struct PhysicalMemoryRegion {
    pub frames: FrameRange,
    pub typ: MemoryRegionType,
}
impl PhysicalMemoryRegion {
    pub fn new(frames: FrameRange, typ: MemoryRegionType) -> PhysicalMemoryRegion {
        PhysicalMemoryRegion { frames, typ }
    }
}
impl Deref for PhysicalMemoryRegion {
    type Target = FrameRange;
    fn deref(&self) -> &FrameRange {
        &self.frames
    }
}

/// Types of physical memory. See each variant's documentation.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MemoryRegionType {
    /// Memory that is available for any general purpose.
    Free,
    // /// Memory that is designated by software for specific purposes.
    // /// Designated memory regions are only allocated from if specifically requested,
    // /// or all other `Free` memory has already been used, i.e., a last resort. 
    // Designated,
    /// Memory that is reserved for special use and is only ever allocated from if specifically requested. 
    /// This includes custom memory regions added by third parties, e.g., 
    /// device memory discovered and added by device drivers later during runtime.
    Reserved,
    /// Memory that is inaccessible and should never be used, ever.
    /// Forbidden regions can never be allocated from. 
    Forbidden,
}

/// A range of contiguous frames.
///
/// # Ordering and Equality
///
/// `Chunk` implements the `Ord` trait, and its total ordering is ONLY based on
/// its **starting** `Frame`. This is useful so we can store `Chunk`s in a sorted collection.
///
/// Similarly, `Chunk` implements equality traits, `Eq` and `PartialEq`,
/// both of which are also based ONLY on the **starting** `Frame` of the `Chunk`.
/// Thus, comparing two `Chunk`s with the `==` or `!=` operators may not work as expected.
/// since it ignores their actual range of frames.
#[derive(Debug, Clone, Eq)]
struct Chunk {
    /// Whether this chunk is in a free, designated, reserved, or forbidden region.
    typ: MemoryRegionType,
    /// The Frames covered by this chunk, an inclusive range. 
    frames: FrameRange,
}
impl Chunk {
    fn as_allocated_frames(&self) -> AllocatedFrames {
        AllocatedFrames {
            frames: self.frames.clone(),
        }
    }

    /// Returns a new `Chunk` with an empty range of frames. 
    fn empty() -> Chunk {
        Chunk {
            typ: MemoryRegionType::Forbidden,
            frames: FrameRange::empty(),
        }
    }
}
impl Deref for Chunk {
    type Target = FrameRange;
    fn deref(&self) -> &FrameRange {
        &self.frames
    }
}
impl Ord for Chunk {
    fn cmp(&self, other: &Self) -> Ordering {
        self.frames.start().cmp(other.frames.start())
    }
}
impl PartialOrd for Chunk {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl PartialEq for Chunk {
    fn eq(&self, other: &Self) -> bool {
        self.frames.start() == other.frames.start()
    }
}
impl Borrow<Frame> for &'_ Chunk {
    fn borrow(&self) -> &Frame {
        self.frames.start()
    }
}


/// Represents a range of allocated `PhysicalAddress`es, specified in `Frame`s. 
/// 
/// These frames are not initially mapped to any physical memory frames, you must do that separately
/// in order to actually use their memory; see the `MappedFrames` type for more. 
/// 
/// This object represents ownership of the allocated physical frames;
/// if this object falls out of scope, its allocated frames will be auto-deallocated upon drop. 
pub struct AllocatedFrames {
    frames: FrameRange,
}

// AllocatedFrames must not be Cloneable, and it must not expose its inner frames as mutable.
assert_not_impl_any!(AllocatedFrames: DerefMut, Clone);

impl Deref for AllocatedFrames {
    type Target = FrameRange;
    fn deref(&self) -> &FrameRange {
        &self.frames
    }
}
impl fmt::Debug for AllocatedFrames {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "AllocatedFrames({:?})", self.frames)
    }
}

impl AllocatedFrames {
    /// Returns an empty AllocatedFrames object that performs no frame allocation. 
    /// Can be used as a placeholder, but will not permit any real usage. 
    pub const fn empty() -> AllocatedFrames {
        AllocatedFrames {
            frames: FrameRange::empty()
        }
    }

    /// Merges the given `AllocatedFrames` object `ap` into this `AllocatedFrames` object (`self`).
    /// This is just for convenience and usability purposes, it performs no allocation or remapping.
    ///
    /// The `ap` must be physically contiguous and come immediately after `self`,
    /// that is, `self.end` must equal `ap.start`. 
    /// If this condition is met, `self` is modified and `Ok(())` is returned,
    /// otherwise `Err(ap)` is returned.
    pub fn merge(&mut self, ap: AllocatedFrames) -> Result<(), AllocatedFrames> {
        // make sure the frames are contiguous
        if *ap.start() != (*self.end() + 1) {
            return Err(ap);
        }
        self.frames = FrameRange::new(*self.start(), *ap.end());
        // ensure the now-merged AllocatedFrames doesn't run its drop handler and free its frames.
        core::mem::forget(ap); 
        Ok(())
    }

    /// Splits this `AllocatedFrames` into two separate `AllocatedFrames` objects:
    /// * `[beginning : at_frame - 1]`
    /// * `[at_frame : end]`
    /// 
    /// Depending on the size of this `AllocatedFrames`, either one of the 
    /// returned `AllocatedFrames` objects may be empty. 
    /// 
    /// Returns an `Err` containing this `AllocatedFrames` if `at_frame` is not within its bounds.
    pub fn split(self, at_frame: Frame) -> Result<(AllocatedFrames, AllocatedFrames), AllocatedFrames> {
        let end_of_first = at_frame - 1;
        if at_frame > *self.frames.start() && end_of_first <= *self.frames.end() {
            let first  = FrameRange::new(*self.frames.start(), end_of_first);
            let second = FrameRange::new(at_frame, *self.frames.end());
            // ensure the original AllocatedFrames doesn't run its drop handler and free its frames.
            core::mem::forget(self); 
            Ok((
                AllocatedFrames { frames: first }, 
                AllocatedFrames { frames: second },
            ))
        } else {
            Err(self)
        }
    }

    /// An escape hatch to create an AllocatedFrames object without actually allocating it. 
    /// Currently used in page table and mapper code only. 
    /// TODO: remove this once we flesh out the rest of the frame deallocation interface.
    #[doc(hidden)]
    pub unsafe fn from_parts_unsafe(frames: FrameRange) -> AllocatedFrames {
        AllocatedFrames { frames }
    }
}

impl Drop for AllocatedFrames {
    fn drop(&mut self) {
        if self.size_in_frames() == 0 { return; }
        trace!("frame_allocator: deallocating {:?}", self);

        // Simply add the newly-deallocated chunk to the free frames list.
        let mut locked_list = FREE_FRAME_LIST.lock();
        let res = locked_list.insert(Chunk {
            typ: todo!("FIXME chunk type in deallocation"),
            frames: self.frames.clone(),
        });
        match res {
            Ok(_inserted_free_chunk) => return,
            Err(c) => error!("BUG: couldn't insert deallocated chunk {:?} into free frame list", c),
        }
        
        // Here, we could optionally use above `_inserted_free_chunk` to merge the adjacent (contiguous) chunks
        // before or after the newly-inserted free chunk. 
        // However, there's no *need* to do so until we actually run out of address space or until 
        // a requested address is in a chunk that needs to be merged.
        // Thus, for performance, we save that for those future situations.
    }
}



/// A series of pending actions related to frame allocator bookkeeping,
/// which may result in heap allocation. 
/// 
/// The actions are triggered upon dropping this struct. 
/// This struct can be returned from the `allocate_frames()` family of functions 
/// in order to allow the caller to precisely control when those actions 
/// that may result in heap allocation should occur. 
/// Such actions include adding chunks to lists of free frames or frames in use. 
/// 
/// The vast majority of use cases don't  care about such precise control, 
/// so you can simply drop this struct at any time or ignore it
/// with a `let _ = ...` binding to instantly drop it. 
pub struct DeferredAllocAction<'list> {
    /// A reference to the list into which we will insert the free `Chunk`s.
    free_list: &'list Mutex<StaticArrayRBTree<Chunk>>,
    /// A free chunk that needs to be added back to the free list.
    free1: Chunk,
    /// Another free chunk that needs to be added back to the free list.
    free2: Chunk,
}
impl<'list> DeferredAllocAction<'list> {
    fn new<F1, F2>(free1: F1, free2: F2) -> DeferredAllocAction<'list> 
        where F1: Into<Option<Chunk>>,
              F2: Into<Option<Chunk>>,
    {
        let free_list = &FREE_FRAME_LIST;
        let free1 = free1.into().unwrap_or(Chunk::empty());
        let free2 = free2.into().unwrap_or(Chunk::empty());
        DeferredAllocAction { free_list, free1, free2 }
    }
}
impl<'list> Drop for DeferredAllocAction<'list> {
    fn drop(&mut self) {
        // Insert all of the chunks, both allocated and free ones, into the list. 
        if self.free1.size_in_frames() > 0 {
            self.free_list.lock().insert(self.free1.clone()).unwrap();
        }
        if self.free2.size_in_frames() > 0 {
            self.free_list.lock().insert(self.free2.clone()).unwrap();
        }
    }
}


/// Possible allocation errors.
enum AllocationError {
    /// The requested address was not free: it was already allocated, or is outside the range of this allocator.
    AddressNotFree(Frame, usize),
    /// The address space was full, or there was not a large-enough chunk 
    /// or enough remaining chunks that could satisfy the requested allocation size.
    OutOfAddressSpace(usize),
}
impl From<AllocationError> for &'static str {
    fn from(alloc_err: AllocationError) -> &'static str {
        match alloc_err {
            AllocationError::AddressNotFree(..) => "address was in use or outside of this allocator's range",
            AllocationError::OutOfAddressSpace(..) => "out of address space",
        }
    }
}


/// Searches the given `list` for the chunk that contains the range of frames from
/// `requested_frame` to `requested_frame + num_frames`.
fn find_specific_chunk(
    list: &mut StaticArrayRBTree<Chunk>,
    requested_frame: Frame,
    num_frames: usize
) -> Result<(AllocatedFrames, DeferredAllocAction<'static>), AllocationError> {

    // The end frame is an inclusive bound, hence the -1. Parentheses are needed to avoid overflow.
    let requested_end_frame = requested_frame + (num_frames - 1); 

    match &mut list.0 {
        Inner::Array(ref mut arr) => {
            for elem in arr.iter_mut() {
                if let Some(chunk) = elem {
                    if requested_frame >= *chunk.frames.start() && requested_end_frame <= *chunk.frames.end() {
                        // Here: `chunk` was big enough and did contain the requested address.
                        return adjust_chosen_chunk(requested_frame, num_frames, &chunk.clone(), ValueRefMut::Array(elem));
                    }
                }
            }
        }
        Inner::RBTree(ref mut tree) => {
            let cursor_mut = tree.upper_bound_mut(Bound::Included(&requested_frame));
            if let Some(chunk) = cursor_mut.get().map(|w| w.deref()) {
                if chunk.contains(&requested_frame) {
                    if requested_end_frame <= *chunk.frames.end() {
                        return adjust_chosen_chunk(requested_frame, num_frames, &chunk.clone(), ValueRefMut::RBTree(cursor_mut));
                    } else {
                        todo!("Frame allocator: found chunk containing requested address, but it was too small. \
                            Merging multiple chunks during an allocation is currently unsupported, please contact the Theseus developers. \
                            Requested address: {:?}, num_frames: {}, chunk: {:?}",
                            requested_frame, num_frames, chunk,
                        );
                    }
                } else {
                    error!("HERE XXX address {:?} was already allocated", requested_frame);
                }
            }
        }
    }

    Err(AllocationError::AddressNotFree(requested_frame, num_frames))
}


/// Searches the given `list` for any chunk large enough to hold at least `num_frames`.
fn find_any_chunk<'list>(
    list: &'list mut StaticArrayRBTree<Chunk>,
    num_frames: usize
) -> Result<(AllocatedFrames, DeferredAllocAction<'static>), AllocationError> {
    // During the first pass, we ignore designated regions.
    match list.0 {
        Inner::Array(ref mut arr) => {
            for elem in arr.iter_mut() {
                if let Some(chunk) = elem {
                    // Skip chunks that are too-small or in the designated regions.
                    if  chunk.size_in_frames() < num_frames || chunk.typ != MemoryRegionType::Free {
                        continue;
                    } 
                    else {
                        return adjust_chosen_chunk(*chunk.start(), num_frames, &chunk.clone(), ValueRefMut::Array(elem));
                    }
                }
            }
        }
        Inner::RBTree(ref mut tree) => {
            // NOTE: if RBTree had a `range_mut()` method, we could simply do the following:
            // ```
            // let eligible_chunks = tree.range(
            //     Bound::Excluded(&DESIGNATED_FRAMES_LOW_END),
            //     Bound::Excluded(&DESIGNATED_FRAMES_HIGH_START)
            // );
            // for c in eligible_chunks { ... }
            // ```
            //
            // However, RBTree doesn't have a `range_mut()` method, so we use cursors for manual iteration.
            //
            // Because we allocate new frames by peeling them off from the beginning part of a chunk, 
            // it's MUCH faster to start the search for free frames from higher addresses moving down. 
            // This results in an O(1) allocation time in the general case, until all address ranges are already in use.
            let mut cursor = tree.upper_bound_mut(Bound::<&Chunk>::Unbounded);
            while let Some(chunk) = cursor.get().map(|w| w.deref()) {
                if num_frames < chunk.size_in_frames() && chunk.typ == MemoryRegionType::Free {
                    return adjust_chosen_chunk(*chunk.start(), num_frames, &chunk.clone(), ValueRefMut::RBTree(cursor));
                }
                warn!("Frame allocator: untested scenario: had to search multiple chunks \
                    (skipping {:?}) while trying to allocate {} frames at any address.",
                    chunk, num_frames
                );
                cursor.move_prev();
            }
        }
    }

    // If we can't find any suitable chunks in the non-designated regions, then look in both designated regions.
    error!("frame_allocator: non-reserved chunks are all allocated (requested {} frames). \
        TODO: other frames need to be freed up here.", num_frames
    );

    Err(AllocationError::OutOfAddressSpace(num_frames))
}


/// The final part of the main allocation routine. 
///
/// The given chunk is the one we've chosen to allocate from. 
/// This function breaks up that chunk into multiple ones and returns an `AllocatedFrames` 
/// from (part of) that chunk, ranging from `start_frame` to `start_frame + num_frames`.
fn adjust_chosen_chunk(
    start_frame: Frame,
    num_frames: usize,
    chosen_chunk: &Chunk,
    mut chosen_chunk_ref: ValueRefMut<Chunk>,
) -> Result<(AllocatedFrames, DeferredAllocAction<'static>), AllocationError> {

    // The new allocated chunk might start in the middle of an existing chunk,
    // so we need to break up that existing chunk into 3 possible chunks: before, newly-allocated, and after.
    //
    // Because Frames and PhysicalAddresses use saturating add and subtract, we need to double-check that we're not creating
    // an overlapping duplicate Chunk at either the very minimum or the very maximum of the address space.
    let new_allocation = Chunk {
        typ: chosen_chunk.typ,
        // The end frame is an inclusive bound, hence the -1. Parentheses are needed to avoid overflow.
        frames: FrameRange::new(start_frame, start_frame + (num_frames - 1)),
    };
    let before = if start_frame == MIN_FRAME {
        None
    } else {
        Some(Chunk {
            typ: chosen_chunk.typ,
            frames: FrameRange::new(*chosen_chunk.frames.start(), *new_allocation.start() - 1),
        })
    };
    let after = if new_allocation.end() == &MAX_FRAME { 
        None
    } else {
        Some(Chunk {
            typ: chosen_chunk.typ,
            frames: FrameRange::new(*new_allocation.end() + 1, *chosen_chunk.frames.end()),
        })
    };

    // some sanity checks -- these can be removed or disabled for better performance
    if let Some(ref b) = before {
        assert!(!new_allocation.contains(b.end()));
        assert!(!b.contains(new_allocation.start()));
    }
    if let Some(ref a) = after {
        assert!(!new_allocation.contains(a.start()));
        assert!(!a.contains(new_allocation.end()));
    }

    // Remove the chosen chunk from the free frame list.
    let _removed_chunk = chosen_chunk_ref.remove();
    assert_eq!(Some(chosen_chunk), _removed_chunk.as_ref()); // sanity check

    // TODO: Re-use the allocated wrapper if possible, rather than allocate a new one entirely.
    // if let RemovedValue::RBTree(Some(wrapper_adapter)) = _removed_chunk { ... }

    Ok((
        new_allocation.as_allocated_frames(),
        DeferredAllocAction::new(before, after),
    ))
}


/// The core frame allocation routine that allocates the given number of physical frames,
/// optionally at the requested starting `PhysicalAddress`.
/// 
/// This simply reserves a range of physical addresses, it does not allocate 
/// actual physical memory frames nor do any memory mapping. 
/// Thus, the returned `AllocatedFrames` aren't directly usable until they are mapped to physical frames. 
/// 
/// Allocation is based on a red-black tree and is thus `O(log(n))`.
/// Fragmentation isn't cleaned up until we're out of address space, but that's not really a big deal.
/// 
/// # Arguments
/// * `requested_paddr`: if `Some`, the returned `AllocatedFrames` will start at the `Frame`
///   containing this `PhysicalAddress`. 
///   If `None`, the first available `Frame` range will be used, starting at any random physical address.
/// * `num_frames`: the number of `Frame`s to be allocated. 
/// 
/// # Return
/// If successful, returns a tuple of two items:
/// * the frames that were allocated, and
/// * an opaque struct representing details of bookkeeping-related actions that may cause heap allocation. 
///   Those actions are deferred until this returned `DeferredAllocAction` struct object is dropped, 
///   allowing the caller (such as the heap implementation itself) to control when heap allocation may occur.
pub fn allocate_frames_deferred(
    requested_paddr: Option<PhysicalAddress>,
    num_frames: usize,
) -> Result<(AllocatedFrames, DeferredAllocAction<'static>), &'static str> {
    if num_frames == 0 {
        warn!("frame_allocator: requested an allocation of 0 frames... stupid!");
        return Err("cannot allocate zero frames");
    }

    let mut locked_list = FREE_FRAME_LIST.lock();

    // The main logic of the allocator is to find an appropriate chunk that can satisfy the allocation request.
    // An appropriate chunk satisfies the following conditions:
    // - Can fit the requested size (starting at the requested address) within the chunk.
    // - The chunk can only be within in a designated region if a specific address was requested, 
    //   or all other non-designated chunks are already in use.
    if let Some(paddr) = requested_paddr {
        find_specific_chunk(&mut locked_list, Frame::containing_address(paddr), num_frames)
    } else {
        find_any_chunk(&mut locked_list, num_frames)
    }.map_err(From::from) // convert from AllocationError to &str
        .map(|(af, _action)| {
            warn!("Allocated {} frames at requested paddr {:?}: {:?}", num_frames, requested_paddr, af);
            (af, _action)
        })
        .map_err(|_e| {
            error!("Failed to allocate {} frames at requested paddr {:?}: {:?}", num_frames, requested_paddr, _e);
            _e
        })
}


/// Similar to [`allocated_frames_deferred()`](fn.allocate_frames_deferred.html),
/// but accepts a size value for the allocated frames in number of bytes instead of number of frames. 
/// 
/// This function still allocates whole frames by rounding up the number of bytes. 
pub fn allocate_frames_by_bytes_deferred(
    requested_paddr: Option<PhysicalAddress>,
    num_bytes: usize,
) -> Result<(AllocatedFrames, DeferredAllocAction<'static>), &'static str> {
    let actual_num_bytes = if let Some(paddr) = requested_paddr {
        num_bytes + (paddr.value() % FRAME_SIZE)
    } else {
        num_bytes
    };
    let num_frames = (actual_num_bytes + FRAME_SIZE - 1) / FRAME_SIZE; // round up
    allocate_frames_deferred(requested_paddr, num_frames)
}


/// Allocates the given number of frames with no constraints on the starting physical address.
/// 
/// See [`allocate_frames_deferred()`](fn.allocate_frames_deferred.html) for more details. 
pub fn allocate_frames(num_frames: usize) -> Option<AllocatedFrames> {
    allocate_frames_deferred(None, num_frames)
        .map(|(ap, _action)| ap)
        .ok()
}


/// Allocates frames with no constraints on the starting physical address, 
/// with a size given by the number of bytes. 
/// 
/// This function still allocates whole frames by rounding up the number of bytes. 
/// See [`allocate_frames_deferred()`](fn.allocate_frames_deferred.html) for more details. 
pub fn allocate_frames_by_bytes(num_bytes: usize) -> Option<AllocatedFrames> {
    allocate_frames_by_bytes_deferred(None, num_bytes)
        .map(|(ap, _action)| ap)
        .ok()
}


/// Allocates frames starting at the given `PhysicalAddress` with a size given in number of bytes. 
/// 
/// This function still allocates whole frames by rounding up the number of bytes. 
/// See [`allocate_frames_deferred()`](fn.allocate_frames_deferred.html) for more details. 
pub fn allocate_frames_by_bytes_at(paddr: PhysicalAddress, num_bytes: usize) -> Result<AllocatedFrames, &'static str> {
    allocate_frames_by_bytes_deferred(Some(paddr), num_bytes)
        .map(|(ap, _action)| ap)
}


/// Allocates the given number of frames starting at (inclusive of) the frame containing the given `PhysicalAddress`.
/// 
/// See [`allocate_frames_deferred()`](fn.allocate_frames_deferred.html) for more details. 
pub fn allocate_frames_at(paddr: PhysicalAddress, num_frames: usize) -> Result<AllocatedFrames, &'static str> {
    allocate_frames_deferred(Some(paddr), num_frames)
        .map(|(ap, _action)| ap)
}


/// Converts the frame allocator from using static memory (a primitive array) to dynamically-allocated memory.
/// 
/// Call this function once heap allocation is available. 
/// Calling this multiple times is unnecessary but harmless, as it will do nothing after the first invocation.
#[doc(hidden)] 
pub fn convert_to_heap_allocated() {
    FREE_FRAME_LIST.lock().convert_to_heap_allocated();
}

/// A debugging function used to dump the full internal state of the frame allocator. 
#[doc(hidden)] 
pub fn dump_frame_allocator_state() {
    debug!("--------------- FREE FRAMES LIST ---------------");
    for c in FREE_FRAME_LIST.lock().iter() {
        debug!("{:X?}", c);
    }
    debug!("---------------------------------------------------");
}
