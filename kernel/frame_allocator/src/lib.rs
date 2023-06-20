//! Provides an allocator for physical memory frames.
//! The minimum unit of allocation is a single frame. 
//!
//! This is currently a modified and more complex version of the `page_allocator` crate.
//! TODO: extract the common code and create a generic allocator that can be specialized to allocate pages or frames.
//! 
//! This also supports early allocation of frames before heap allocation is available, 
//! and does so behind the scenes using the same single interface. 
//! Early pre-heap allocations are limited to tracking a small number of available chunks (currently 32).
//! 
//! Once heap allocation is available, it uses a dynamically-allocated list of frame chunks to track allocations.
//! 
//! The core allocation function is [`allocate_frames_deferred()`](fn.allocate_frames_deferred.html), 
//! but there are several convenience functions that offer simpler interfaces for general usage. 
//!
//! # Notes and Missing Features
//! This allocator currently does **not** merge freed chunks (de-fragmentation). 
//! We don't need to do so until we actually run out of address space or until 
//! a requested address is in a chunk that needs to be merged.

#![allow(clippy::blocks_in_if_conditions)]
#![no_std]
#![feature(box_into_inner)]
#![allow(incomplete_features)]
#![feature(adt_const_params)]

extern crate alloc;
#[macro_use] extern crate log;
extern crate kernel_config;
extern crate memory_structs;
extern crate spin;
#[macro_use] extern crate static_assertions;
extern crate intrusive_collections;
extern crate range_inclusive;
extern crate trusted_chunk;
#[cfg(test)]
mod test;

mod static_array_rb_tree;
// mod static_array_linked_list;
mod frames;

use core::{borrow::Borrow, cmp::{Ordering, min, max}, ops::Deref};
use frames::*;
use kernel_config::memory::*;
use memory_structs::{PhysicalAddress, Frame, FrameRange};
use spin::Mutex;
use intrusive_collections::Bound;
use static_array_rb_tree::*;
use trusted_chunk::trusted_chunk::TrustedChunk;
use range_inclusive::RangeInclusive;
pub use frames::{AllocatedFrames, UnmappedFrame};

const FRAME_SIZE: usize = PAGE_SIZE;
#[allow(dead_code)]
const MIN_FRAME: Frame = Frame::containing_address(PhysicalAddress::zero());
#[allow(dead_code)]
const MAX_FRAME: Frame = Frame::containing_address(PhysicalAddress::new_canonical(usize::MAX));

// Note: we keep separate lists for "free, general-purpose" areas and "reserved" areas, as it's much faster. 

/// The single, system-wide list of free physical memory frames available for general usage. 
static FREE_GENERAL_FRAMES_LIST: Mutex<StaticArrayRBTree<Frames<{FrameState::Unmapped}>>> = Mutex::new(StaticArrayRBTree::empty()); 
/// The single, system-wide list of free physical memory frames reserved for specific usage. 
static FREE_RESERVED_FRAMES_LIST: Mutex<StaticArrayRBTree<Frames<{FrameState::Unmapped}>>> = Mutex::new(StaticArrayRBTree::empty()); 

/// The fixed list of all known regions that are available for general use.
/// This does not indicate whether these regions are currently allocated, 
/// rather just where they exist and which regions are known to this allocator.
static GENERAL_REGIONS: Mutex<StaticArrayRBTree<Region>> = Mutex::new(StaticArrayRBTree::empty());
/// The fixed list of all known regions that are reserved for specific purposes. 
/// This does not indicate whether these regions are currently allocated, 
/// rather just where they exist and which regions are known to this allocator.
static RESERVED_REGIONS: Mutex<StaticArrayRBTree<Region>> = Mutex::new(StaticArrayRBTree::empty());

type IntoTrustedChunkFn = fn(RangeInclusive<usize>) -> TrustedChunk;
type IntoAllocatedFramesFn = fn(TrustedChunk, FrameRange) -> Frames<{FrameState::Unmapped}>;

/// Initialize the frame allocator with the given list of available and reserved physical memory regions.
///
/// Any regions in either of the lists may overlap, this is checked for and handled properly.
/// Reserved regions take priority -- if a reserved region partially or fully overlaps any part of a free region,
/// that portion will be considered reserved, not free. 
/// 
/// The iterator (`R`) over reserved physical memory regions must be cloneable, 
/// as this runs before heap allocation is available, and we may need to iterate over it multiple times. 
/// 
/// ## Return
/// Upon success, this function returns a callback function that allows the caller
/// (the memory subsystem init function) to convert a range of unmapped frames 
/// back into an [`Frames<{FrameState::Unmapped}>`] object.
pub fn init<F, R, P>(
    free_physical_memory_areas: F,
    reserved_physical_memory_areas: R,
) -> Result<(IntoTrustedChunkFn, IntoAllocatedFramesFn), &'static str> 
    where P: Borrow<PhysicalMemoryRegion>,
          F: IntoIterator<Item = P>,
          R: IntoIterator<Item = P> + Clone,
{
    if  FREE_GENERAL_FRAMES_LIST .lock().len() != 0 ||
        FREE_RESERVED_FRAMES_LIST.lock().len() != 0 ||
        GENERAL_REGIONS          .lock().len() != 0 ||
        RESERVED_REGIONS         .lock().len() != 0 
    {
        return Err("BUG: Frame allocator was already initialized, cannot be initialized twice.");
    }

    // start with all lists using the `Region` type so we can merge and manipulate until we're sure we have non-overlapping regions
    let mut free_list: [Option<Region>; 32] = Default::default();
    let mut free_list_idx = 0;
    
    // Populate the list of free regions for general-purpose usage.
    for area in free_physical_memory_areas.into_iter() {
        let area = area.borrow();
        // debug!("Frame Allocator: looking to add free physical memory area: {:?}", area);
        check_and_add_free_region(
            area,
            &mut free_list,
            &mut free_list_idx,
            reserved_physical_memory_areas.clone(),
        );
    }
    

    let mut reserved_list: [Option<Region>; 32] = Default::default();
    for (i, area) in reserved_physical_memory_areas.into_iter().enumerate() {
        reserved_list[i] = Some(Region {
            typ: MemoryRegionType::Reserved,
            frames: area.borrow().frames.clone(),
        });
    }

    let mut changed = true;
    while changed {
        let mut temp_reserved_list: [Option<Region>; 32] = Default::default();
        changed = false;
        
        let mut temp_reserved_list_idx = 0;
        for i in 0..temp_reserved_list.len() {
            if let Some(mut current) = reserved_list[i].clone() {
                for maybe_other in &mut reserved_list[i + 1..] {
                    if let Some(other) = maybe_other {
                        if current.overlap(other).is_some() {
                            current.frames = FrameRange::new(
                                min(*current.start(), *other.start()),
                                max(*current.end(), *other.end()),
                            );

                            changed = true;
                            *maybe_other = None;
                        }
                    }
                }
                temp_reserved_list[temp_reserved_list_idx] = Some(current);
                temp_reserved_list_idx += 1;
            }
        }
        
        reserved_list = temp_reserved_list;
    }
    
    // We can remove this sanity check because the following code uses formally verified functions to ensure no two regions overlap.
    // // Finally, one last sanity check -- ensure no two regions overlap. 
    // let all_areas = free_list[..free_list_idx].iter().flatten()
    // .chain(reserved_list.iter().flatten());
    // for (i, elem) in all_areas.clone().enumerate() {
    //     let next_idx = i + 1;
    //     for other in all_areas.clone().skip(next_idx) {
    //         if let Some(overlap) = elem.overlap(other) {
    //             panic!("BUG: frame allocator free list had overlapping ranges: \n \t {:?} and {:?} overlap at {:?}",
    //                 elem, other, overlap,
    //             );
    //         }
    //     }
    // }

    // Here, since we're sure we now have a list of regions that don't overlap, we can create lists of formally verified Chunks
    let mut free_list_w_chunks: [Option<Frames<{FrameState::Unmapped}>>; 32] = Default::default();
    let mut reserved_list_w_chunks: [Option<Frames<{FrameState::Unmapped}>>; 32] = Default::default();
    for (i, elem) in reserved_list.iter().flatten().enumerate() {
        reserved_list_w_chunks[i] = Some(Frames::new(
            MemoryRegionType::Reserved,
            elem.frames.clone()
        )?);
    }

    for (i, elem) in free_list.iter().flatten().enumerate() {
        free_list_w_chunks[i] = Some(Frames::new(
            MemoryRegionType::Free,
            elem.frames.clone()
        )?);
    }

    *FREE_GENERAL_FRAMES_LIST.lock()  = StaticArrayRBTree::new(free_list_w_chunks);
    *FREE_RESERVED_FRAMES_LIST.lock() = StaticArrayRBTree::new(reserved_list_w_chunks);
    *GENERAL_REGIONS.lock()           = StaticArrayRBTree::new(free_list);
    *RESERVED_REGIONS.lock()          = StaticArrayRBTree::new(reserved_list);

    // Register the callbacks to create a TrustedChunk and AllocatedFrames from an unmapped PTE
    Ok((trusted_chunk::init()?, frames::into_allocated_frames))
}


/// The main logic of the initialization routine 
/// used to populate the list of free frame chunks.
///
/// This function recursively iterates over the given `area` of frames
/// and adds any ranges of frames within that `area` that are not covered by
/// the given list of `reserved_physical_memory_areas`.
fn check_and_add_free_region<P, R>(
    area: &FrameRange,
    free_list: &mut [Option<Region>; 32],
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
    // trace!("looking at sub-area {:X?} to {:X?}", current_start, current_end);

    for reserved in reserved_physical_memory_areas.clone().into_iter() {
        let reserved = &reserved.borrow().frames;
        // trace!("\t Comparing with reserved area {:X?}", reserved);
        if reserved.contains(&current_start) {
            // info!("\t\t moving current_start from {:X?} to {:X?}", current_start, *reserved.end() + 1);
            current_start = *reserved.end() + 1;
        }
        if &current_start <= reserved.start() && reserved.start() <= &current_end {
            // Advance up to the frame right before this reserved region started.
            // info!("\t\t moving current_end from {:X?} to {:X?}", current_end, min(current_end, *reserved.start() - 1));
            current_end = min(current_end, *reserved.start() - 1);
            if area.end() <= reserved.end() {
                // Optimization here: the rest of the current area is reserved,
                // so there's no need to keep iterating over the reserved areas.
                // info!("\t !!! skipping the rest of the area");
                break;
            } else {
                let after = FrameRange::new(*reserved.end() + 1, *area.end());
                // warn!("moving on to after {:X?}", after);
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
        free_list[*free_list_idx] = Some(Region {
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
    /// Memory that is reserved for special use and is only ever allocated from if specifically requested. 
    /// This includes custom memory regions added by third parties, e.g., 
    /// device memory discovered and added by device drivers later during runtime.
    Reserved,
    /// Memory of an unknown type.
    /// This is a default value that acts as a sanity check, because it is invalid
    /// to do any real work (e.g., allocation, access) with an unknown memory region.
    Unknown,
}

/// A region of contiguous frames.
/// Only used for bookkeeping, not for allocation.
///
/// # Ordering and Equality
///
/// `Region` implements the `Ord` trait, and its total ordering is ONLY based on
/// its **starting** `Frame`. This is useful so we can store `Region`s in a sorted collection.
///
/// Similarly, `Region` implements equality traits, `Eq` and `PartialEq`,
/// both of which are also based ONLY on the **starting** `Frame` of the `Region`.
/// Thus, comparing two `Region`s with the `==` or `!=` operators may not work as expected.
/// since it ignores their actual range of frames.
#[derive(Debug, Clone, Eq)]
#[allow(dead_code)]
pub struct Region {
    /// The type of this memory region, e.g., whether it's in a free or reserved region.
    typ: MemoryRegionType,
    /// The Frames covered by this region, an inclusive range. 
    frames: FrameRange,
}
impl Region {
    /// Returns a new `Region` with an empty range of frames. 
    pub fn empty() -> Region {
        Region {
            typ: MemoryRegionType::Unknown,
            frames: FrameRange::empty(),
        }
    }
}

impl Deref for Region {
    type Target = FrameRange;
    fn deref(&self) -> &FrameRange {
        &self.frames
    }
}
impl Ord for Region {
    fn cmp(&self, other: &Self) -> Ordering {
        self.frames.start().cmp(other.frames.start())
    }
}
impl PartialOrd for Region {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl PartialEq for Region {
    fn eq(&self, other: &Self) -> bool {
        self.frames.start() == other.frames.start()
    }
}
impl Borrow<Frame> for &'_ Region {
    fn borrow(&self) -> &Frame {
        self.frames.start()
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
/// The vast majority of use cases don't care about such precise control, 
/// so you can simply drop this struct at any time or ignore it
/// with a `let _ = ...` binding to instantly drop it. 
pub struct DeferredAllocAction<'list> {
    /// A reference to the list into which we will insert the free general-purpose `Chunk`s.
    free_list: &'list Mutex<StaticArrayRBTree<Frames<{FrameState::Unmapped}>>>,
    /// A reference to the list into which we will insert the free "reserved" `Chunk`s.
    reserved_list: &'list Mutex<StaticArrayRBTree<Frames<{FrameState::Unmapped}>>>,
    /// A free chunk that needs to be added back to the free list.
    free1: Frames<{FrameState::Unmapped}>,
    /// Another free chunk that needs to be added back to the free list.
    free2: Frames<{FrameState::Unmapped}>,
}
impl<'list> DeferredAllocAction<'list> {
    fn new<F1, F2>(free1: F1, free2: F2) -> DeferredAllocAction<'list> 
        where F1: Into<Option<Frames<{FrameState::Unmapped}>>>,
              F2: Into<Option<Frames<{FrameState::Unmapped}>>>,
    {
        let free1 = free1.into().unwrap_or_else(Frames::empty);
        let free2 = free2.into().unwrap_or_else(Frames::empty);
        DeferredAllocAction {
            free_list: &FREE_GENERAL_FRAMES_LIST,
            reserved_list: &FREE_RESERVED_FRAMES_LIST,
            free1,
            free2
        }
    }
}
impl<'list> Drop for DeferredAllocAction<'list> {
    fn drop(&mut self) {
        let chunk1 = core::mem::replace(&mut self.free1, Frames::empty());
        let chunk2 = core::mem::replace(&mut self.free2, Frames::empty());

        // Insert all of the chunks, both allocated and free ones, into the list. 
        if chunk1.size_in_frames() > 0 {
            match chunk1.typ() {
                MemoryRegionType::Free     => { self.free_list.lock().insert(chunk1).unwrap(); }
                MemoryRegionType::Reserved => { self.reserved_list.lock().insert(chunk1).unwrap(); }
                _ => error!("BUG likely: DeferredAllocAction encountered free1 chunk {:?} of a type Unknown", chunk1),
            }
        }
        if chunk2.size_in_frames() > 0 {
            match chunk2.typ() {
                MemoryRegionType::Free     => { self.free_list.lock().insert(chunk2).unwrap(); }
                MemoryRegionType::Reserved => { self.reserved_list.lock().insert(chunk2).unwrap(); }
                _ => error!("BUG likely: DeferredAllocAction encountered free2 chunk {:?} of a type Unknown", chunk2),
            };
        }
    }
}


/// Possible allocation errors.
#[derive(Debug)]
enum AllocationError {
    /// The requested address was not free: it was already allocated.
    AddressNotFree(Frame, usize),
    /// The requested address was outside the range of this allocator.
    AddressNotFound(Frame, usize),
    /// The address space was full, or there was not a large-enough chunk 
    /// or enough remaining chunks that could satisfy the requested allocation size.
    OutOfAddressSpace(usize),
    /// The starting address was found, but not all successive contiguous frames were available.
    ContiguousChunkNotFound(Frame, usize),
    /// Failed to remove a chunk from the free list given a reference to it.
    ChunkRemovalFailed,
    /// Failed to merge or split a Chunk.
    ChunkOperationFailed,
}
impl From<AllocationError> for &'static str {
    fn from(alloc_err: AllocationError) -> &'static str {
        match alloc_err {
            AllocationError::AddressNotFree(..) => "requested address was in use",
            AllocationError::AddressNotFound(..) => "requested address was outside of this frame allocator's range",
            AllocationError::OutOfAddressSpace(..) => "out of physical address space",
            AllocationError::ContiguousChunkNotFound(..) => "only some of the requested frames were available",
            AllocationError::ChunkRemovalFailed => "Failed to remove a Chunk from the free list, this is most likely due to some logical error",
            AllocationError::ChunkOperationFailed => "A verified chunk function returned an error, this is most likely due to some logical error",
        }
    }
}


/// Searches the given `list` for the chunk that contains the range of frames from
/// `requested_frame` to `requested_frame + num_frames`.
fn find_specific_chunk(
    list: &mut StaticArrayRBTree<Frames<{FrameState::Unmapped}>>,
    requested_frame: Frame,
    num_frames: usize
) -> Result<(Frames<{FrameState::Unmapped}>, DeferredAllocAction<'static>), AllocationError> {

    // The end frame is an inclusive bound, hence the -1. Parentheses are needed to avoid overflow.
    let requested_end_frame = requested_frame + (num_frames - 1);

    match &mut list.0 {
        Inner::Array(ref mut arr) => {
            for elem in arr.iter_mut() {
                if let Some(chunk) = elem {
                    if requested_frame >= *chunk.start() && requested_end_frame <= *chunk.end() {
                        // Here: `chunk` was big enough and did contain the requested address.
                        return allocate_from_chosen_chunk(requested_frame, num_frames, ValueRefMut::Array(elem));
                    }
                }
            }
        }
        Inner::RBTree(ref mut tree) => {
            let cursor_mut = tree.upper_bound_mut(Bound::Included(&requested_frame));
            if let Some(chunk) = cursor_mut.get().map(|w| w.deref()) {
                if chunk.contains(&requested_frame) {
                    if requested_end_frame <= *chunk.end() {
                        return allocate_from_chosen_chunk(requested_frame, num_frames, ValueRefMut::RBTree(cursor_mut));
                    } else {
                        // We found the chunk containing the requested address, but it was too small to cover all of the requested frames.
                        // Let's try to merge the next-highest contiguous chunk to see if those two chunks together 
                        // cover enough frames to fulfill the allocation request.
                        //
                        // trace!("Frame allocator: found chunk containing requested address, but it was too small. \
                        //     Attempting to merge multiple chunks during an allocation. \
                        //     Requested address: {:?}, num_frames: {}, chunk: {:?}",
                        //     requested_frame, num_frames, chunk,
                        // );
                        let initial_chunk_ref: Option<ValueRefMut<Frames<{FrameState::Unmapped}>>> = {
                            let next_cursor = cursor_mut.peek_next();
                            if let Some(next_chunk) = next_cursor.get().map(|w| w.deref()) {
                                if *chunk.end() + 1 == *next_chunk.start() {
                                    // Here: next chunk was contiguous with the original chunk. 
                                    if requested_end_frame <= *next_chunk.end() {
                                        // trace!("Frame allocator: found suitably-large contiguous next {:?} after initial too-small {:?}", next_chunk, chunk);
                                        // We cannot clone a Chunk, so we return a reference to the first chunk,
                                        // so that it can be removed and then we can remove the next chunk.
                                        Some(ValueRefMut::RBTree(cursor_mut))
                                    } else {
                                        todo!("Frame allocator: found chunk containing requested address, but it was too small. \
                                            Theseus does not yet support merging more than two chunks during an allocation request. \
                                            Requested address: {:?}, num_frames: {}, chunk: {:?}, next_chunk {:?}",
                                            requested_frame, num_frames, chunk, next_chunk
                                        );
                                        // None
                                    }
                                } else {
                                    trace!("Frame allocator: next {:?} was not contiguously above initial too-small {:?}", next_chunk, chunk);
                                    None
                                }
                            } else {
                                trace!("Frame allocator: couldn't get next chunk above initial too-small {:?}", chunk);
                                trace!("Requesting new chunk starting at {:?}, num_frames: {}", *chunk.end() + 1, requested_end_frame.number() - chunk.end().number());
                                return Err(AllocationError::ContiguousChunkNotFound(*chunk.end() + 1, requested_end_frame.number() - chunk.end().number()));
                            }
                        };

                        if let Some(initial_chunk_ref) = initial_chunk_ref {
                            // remove the first chunk
                            let initial_chunk = retrieve_chunk_from_ref(initial_chunk_ref).ok_or(AllocationError::ChunkRemovalFailed)?;
                            
                            // now search for the next contiguous chunk, that we already know exists
                            let requested_contiguous_frame = *initial_chunk.end() + 1;
                            let cursor_mut = tree.upper_bound_mut(Bound::Included(&requested_contiguous_frame));
                            if let Some(next_chunk) = cursor_mut.get().map(|w| w.deref()) {
                                if next_chunk.contains(&requested_contiguous_frame) {
                                    // merge the next chunk into the initial chunk
                                    return adjust_chosen_chunk_contiguous(requested_frame, num_frames, initial_chunk, ValueRefMut::RBTree(cursor_mut));
                                } else {
                                    trace!("This should never fail, since we've already found a contiguous chunk.");
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Err(AllocationError::AddressNotFound(requested_frame, num_frames))
}


/// Searches the given `list` for any chunk large enough to hold at least `num_frames`.
fn find_any_chunk(
    list: &mut StaticArrayRBTree<Frames<{FrameState::Unmapped}>>,
    num_frames: usize
) -> Result<(Frames<{FrameState::Unmapped}>, DeferredAllocAction<'static>), AllocationError> {
    // During the first pass, we ignore designated regions.
    match list.0 {
        Inner::Array(ref mut arr) => {
            for elem in arr.iter_mut() {
                if let Some(chunk) = elem {
                    // Skip chunks that are too-small or in the designated regions.
                    if  chunk.size_in_frames() < num_frames || chunk.typ() != MemoryRegionType::Free {
                        continue;
                    } 
                    else {
                        return allocate_from_chosen_chunk(*chunk.start(), num_frames,  ValueRefMut::Array(elem));
                    }
                }
            }
        }
        Inner::RBTree(ref mut tree) => {
            // Because we allocate new frames by peeling them off from the beginning part of a chunk, 
            // it's MUCH faster to start the search for free frames from higher addresses moving down. 
            // This results in an O(1) allocation time in the general case, until all address ranges are already in use.
            let mut cursor = tree.upper_bound_mut(Bound::<&Frames<{FrameState::Unmapped}>>::Unbounded);
            while let Some(chunk) = cursor.get().map(|w| w.deref()) {
                if num_frames <= chunk.size_in_frames() && chunk.typ() == MemoryRegionType::Free {
                    return allocate_from_chosen_chunk(*chunk.start(), num_frames, ValueRefMut::RBTree(cursor));
                }
                warn!("Frame allocator: inefficient scenario: had to search multiple chunks \
                    (skipping {:?}) while trying to allocate {} frames at any address.",
                    chunk, num_frames
                );
                cursor.move_prev();
            }
        }
    }

    error!("frame_allocator: non-reserved chunks are all allocated (requested {} frames). \
        TODO: we could attempt to merge free chunks here.", num_frames
    );

    Err(AllocationError::OutOfAddressSpace(num_frames))
}


/// Removes a chunk from the RBTree. 
/// `chosen_chunk_ref` is basically a wrapper over the cursor which stores the position of the chosen_chunk.
fn retrieve_chunk_from_ref(mut chosen_chunk_ref: ValueRefMut<Frames<{FrameState::Unmapped}>>) -> Option<Frames<{FrameState::Unmapped}>> {
    // Remove the chosen chunk from the free frame list.
    let removed_val = chosen_chunk_ref.remove();
    
    match removed_val {
        RemovedValue::Array(c) => c,
        RemovedValue::RBTree(option_chunk) => {
            option_chunk.map(|c| c.into_inner())
        }
    }
}

/// The final part of the main allocation routine that splits the given chosen chunk
/// into multiple smaller chunks, thereby "allocating" frames from it.
///
/// This function breaks up that chunk into multiple ones and returns an `Frames<{FrameState::Unmapped}>` 
/// from (part of) that chunk, ranging from `start_frame` to `start_frame + num_frames`.
fn allocate_from_chosen_chunk(
    start_frame: Frame,
    num_frames: usize,
    chosen_chunk_ref: ValueRefMut<Frames<{FrameState::Unmapped}>>,
) -> Result<(Frames<{FrameState::Unmapped}>, DeferredAllocAction<'static>), AllocationError> {
    // Remove the chosen chunk from the free frame list.
    let chosen_chunk = retrieve_chunk_from_ref(chosen_chunk_ref).ok_or(AllocationError::ChunkRemovalFailed)?;

    let (new_allocation, before, after) = chosen_chunk.split(start_frame, num_frames);

    // TODO: Re-use the allocated wrapper if possible, rather than allocate a new one entirely.
    // if let RemovedValue::RBTree(Some(wrapper_adapter)) = _removed_chunk { ... }

    Ok((
        new_allocation, //.into_allocated_frames(),
        DeferredAllocAction::new(before, after),
    ))

}

/// Merges the contiguous chunk given by `chunk2_ref` into `chunk1`.
/// Then allocates from the newly merged chunk.
fn adjust_chosen_chunk_contiguous(
    start_frame: Frame,
    num_frames: usize,
    mut initial_chunk: Frames<{FrameState::Unmapped}>,
    contiguous_chunk_ref: ValueRefMut<Frames<{FrameState::Unmapped}>>,
) -> Result<(Frames<{FrameState::Unmapped}>, DeferredAllocAction<'static>), AllocationError> {
    let contiguous_chunk = retrieve_chunk_from_ref(contiguous_chunk_ref).ok_or(AllocationError::ChunkRemovalFailed)?;

    initial_chunk.merge(contiguous_chunk).map_err(|_| {
        trace!("contiguous chunks couldn't be merged, despite previous checks");
        //To Do: should we reinsert chunk to list here.
        AllocationError:: ChunkOperationFailed
    })?;
    let (new_allocation, before, after) = initial_chunk.split(start_frame, num_frames);



    Ok((
        new_allocation, //.into_allocated_frames(),
        DeferredAllocAction::new(before, after),
    ))
}

/// Returns `true` if the given list contains *any* of the given `frames`.
fn contains_any(
    list: &StaticArrayRBTree<Region>,
    frames: &FrameRange,
) -> bool {
    match &list.0 {
        Inner::Array(ref arr) => {
            for chunk in arr.iter().flatten() {
                if chunk.overlap(frames).is_some() {
                    return true;
                }
            }
        }
        Inner::RBTree(ref tree) => {
            let mut cursor = tree.upper_bound(Bound::Included(frames.start()));
            while let Some(chunk) = cursor.get() {
                if chunk.start() > frames.end() {
                    // We're iterating in ascending order over a sorted tree, so we can stop
                    // looking for overlapping regions once we pass the end of `frames`.
                    break;
                }

                if chunk.overlap(frames).is_some() {
                    return true;
                }
                cursor.move_next();
            }
        }
    }
    false
}


/// Adds the given `frames` to the given `list` as a Chunk of reserved frames. 
/// 
/// Returns the range of **new** frames that were added to the list, 
/// which will be a subset of the given input `frames`.
///
/// Currently, this function adds no new frames at all if any frames within the given `frames` list
/// overlap any existing regions at all. 
/// TODO: handle partially-overlapping regions by extending existing regions on either end.
fn add_reserved_region_to_chunk_list(
    list: &mut StaticArrayRBTree<Frames<{FrameState::Unmapped}>>,
    frames: FrameRange,
) -> Result<FrameRange, &'static str> {
    // We can remove this check because creating a Chunk will check for overlaps

    // // Check whether the reserved region overlaps any existing regions.
    // match &mut list.0 {
    //     Inner::Array(ref mut arr) => {
    //         for chunk in arr.iter().flatten() {
    //             if let Some(_overlap) = chunk.overlap(&frames) {
    //                 // trace!("Failed to add reserved region {:?} due to overlap {:?} with existing chunk {:?}",
    //                 //     frames, _overlap, chunk
    //                 // );
    //                 return Err("Failed to add reserved region that overlapped with existing reserved regions (array).");
    //             }
    //         }
    //     }
    //     Inner::RBTree(ref mut tree) => {
    //         let mut cursor_mut = tree.upper_bound_mut(Bound::Included(frames.start()));
    //         while let Some(chunk) = cursor_mut.get().map(|w| w.deref()) {
    //             if chunk.start() > frames.end() {
    //                 // We're iterating in ascending order over a sorted tree,
    //                 // so we can stop looking for overlapping regions once we pass the end of the new frames to add.
    //                 break;
    //             }
    //             if let Some(_overlap) = chunk.overlap(&frames) {
    //                 // trace!("Failed to add reserved region {:?} due to overlap {:?} with existing chunk {:?}",
    //                 //     frames, _overlap, chunk
    //                 // );
    //                 return Err("Failed to add reserved region that overlapped with existing reserved regions (RBTree).");
    //             }
    //             cursor_mut.move_next();
    //         }
    //     }
    // }

    list.insert(Frames::new(
        MemoryRegionType::Reserved,
        frames.clone(),
    )?).map_err(|_c| "BUG: Failed to insert non-overlapping frames into list.")?;

    Ok(frames)
}


/// Adds the given `frames` to the given `list` as a Chunk of reserved frames. 
/// 
/// Returns the range of **new** frames that were added to the list, 
/// which will be a subset of the given input `frames`.
///
/// Currently, this function adds no new frames at all if any frames within the given `frames` list
/// overlap any existing regions at all. 
/// Handling partially-overlapping regions 
fn add_reserved_region_to_region_list(
    list: &mut StaticArrayRBTree<Region>,
    frames: FrameRange,
) -> Result<FrameRange, &'static str> {

    // Check whether the reserved region overlaps any existing regions.
    match &mut list.0 {
        Inner::Array(ref mut arr) => {
            for chunk in arr.iter().flatten() {
                if let Some(_overlap) = chunk.overlap(&frames) {
                    // trace!("Failed to add reserved region {:?} due to overlap {:?} with existing chunk {:?}",
                    //     frames, _overlap, chunk
                    // );
                    return Err("Failed to add reserved region that overlapped with existing reserved regions (array).");
                }
            }
        }
        Inner::RBTree(ref mut tree) => {
            let mut cursor_mut = tree.upper_bound_mut(Bound::Included(frames.start()));
            while let Some(chunk) = cursor_mut.get().map(|w| w.deref()) {
                if chunk.start() > frames.end() {
                    // We're iterating in ascending order over a sorted tree,
                    // so we can stop looking for overlapping regions once we pass the end of the new frames to add.
                    break;
                }
                if let Some(_overlap) = chunk.overlap(&frames) {
                    // trace!("Failed to add reserved region {:?} due to overlap {:?} with existing chunk {:?}",
                    //     frames, _overlap, chunk
                    // );
                    return Err("Failed to add reserved region that overlapped with existing reserved regions (RBTree).");
                }
                cursor_mut.move_next();
            }
        }
    }

    list.insert(Region {
        typ: MemoryRegionType::Reserved,
        frames: frames.clone(),
    }).map_err(|_c| "BUG: Failed to insert non-overlapping frames into list.")?;

    Ok(frames)
}


/// The core frame allocation routine that allocates the given number of physical frames,
/// optionally at the requested starting `PhysicalAddress`.
/// 
/// This simply reserves a range of frames; it does not perform any memory mapping. 
/// Thus, the memory represented by the returned `Frames<{FrameState::Unmapped}>` isn't directly accessible
/// until you map virtual pages to them.
/// 
/// Allocation is based on a red-black tree and is thus `O(log(n))`.
/// Fragmentation isn't cleaned up until we're out of address space, but that's not really a big deal.
/// 
/// # Arguments
/// * `requested_paddr`: if `Some`, the returned `Frames<{FrameState::Unmapped}>` will start at the `Frame`
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
) -> Result<(Frames<{FrameState::Unmapped}>, DeferredAllocAction<'static>), &'static str> {
    if num_frames == 0 {
        warn!("frame_allocator: requested an allocation of 0 frames... stupid!");
        return Err("cannot allocate zero frames");
    }
    
    if let Some(paddr) = requested_paddr {
        let start_frame = Frame::containing_address(paddr);
        let mut free_reserved_frames_list = FREE_RESERVED_FRAMES_LIST.lock();
        // First, attempt to allocate the requested frames from the free reserved list.
        let first_allocation_attempt = find_specific_chunk(&mut free_reserved_frames_list, start_frame, num_frames);
        let (requested_start_frame, requested_num_frames) = match first_allocation_attempt {
            Ok(success) => return Ok(success),
            Err(alloc_err) => match alloc_err {
                AllocationError::AddressNotFound(..) => {
                    // If allocation failed, then the requested `start_frame` may be found in the general-purpose list
                    match find_specific_chunk(&mut FREE_GENERAL_FRAMES_LIST.lock(), start_frame, num_frames) {
                        Ok(result) => return Ok(result),
                        Err(AllocationError::AddressNotFound(..)) => (start_frame, num_frames),
                        Err(AllocationError::ContiguousChunkNotFound(..)) => {
                            // because we are searching the general frames list, it doesn't matter if part of the chunk was found
                            // since we only create new reserved frames.
                            trace!("Only part of the requested allocation was found in the general frames list.");
                            return Err(alloc_err).map_err(From::from);
                        }
                        Err(_other) => return Err(alloc_err).map_err(From::from),
                    }
                },
                AllocationError::ContiguousChunkNotFound(f, numf) => (f, numf),
                _ => return Err(alloc_err).map_err(From::from),
            }
        };

        // If we failed to allocate the requested frames from the general list,
        // we can add a new reserved region containing them,
        // but ONLY if those frames are *NOT* in the general-purpose region.
        let requested_frames = FrameRange::new(requested_start_frame, requested_start_frame + (requested_num_frames - 1));
        if !contains_any(&GENERAL_REGIONS.lock(), &requested_frames) {
            // If we successfully create a new Chunk with verified functions, then add a new reserved region
            let new_free_reserved_frames = add_reserved_region_to_chunk_list(&mut free_reserved_frames_list, requested_frames)?;
            let _new_reserved_frames = add_reserved_region_to_region_list(&mut RESERVED_REGIONS.lock(), new_free_reserved_frames.clone())?;    
            assert_eq!(_new_reserved_frames, new_free_reserved_frames);
            find_specific_chunk(&mut free_reserved_frames_list, start_frame, num_frames)
        } 
        else {
            Err(AllocationError::AddressNotFree(start_frame, num_frames))
        }
    } else {
        find_any_chunk(&mut FREE_GENERAL_FRAMES_LIST.lock(), num_frames)
    }.map_err(From::from) // convert from AllocationError to &str
}


/// Similar to [`allocated_frames_deferred()`](fn.allocate_frames_deferred.html),
/// but accepts a size value for the allocated frames in number of bytes instead of number of frames. 
/// 
/// This function still allocates whole frames by rounding up the number of bytes. 
pub fn allocate_frames_by_bytes_deferred(
    requested_paddr: Option<PhysicalAddress>,
    num_bytes: usize,
) -> Result<(Frames<{FrameState::Unmapped}>, DeferredAllocAction<'static>), &'static str> {
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
pub fn allocate_frames(num_frames: usize) -> Option<Frames<{FrameState::Unmapped}>> {
    allocate_frames_deferred(None, num_frames)
        .map(|(af, _action)| af)
        .ok()
}


/// Allocates frames with no constraints on the starting physical address, 
/// with a size given by the number of bytes. 
/// 
/// This function still allocates whole frames by rounding up the number of bytes. 
/// See [`allocate_frames_deferred()`](fn.allocate_frames_deferred.html) for more details. 
pub fn allocate_frames_by_bytes(num_bytes: usize) -> Option<Frames<{FrameState::Unmapped}>> {
    allocate_frames_by_bytes_deferred(None, num_bytes)
        .map(|(af, _action)| af)
        .ok()
}


/// Allocates frames starting at the given `PhysicalAddress` with a size given in number of bytes. 
/// 
/// This function still allocates whole frames by rounding up the number of bytes. 
/// See [`allocate_frames_deferred()`](fn.allocate_frames_deferred.html) for more details. 
pub fn allocate_frames_by_bytes_at(paddr: PhysicalAddress, num_bytes: usize) -> Result<Frames<{FrameState::Unmapped}>, &'static str> {
    allocate_frames_by_bytes_deferred(Some(paddr), num_bytes)
        .map(|(af, _action)| af)
}


/// Allocates the given number of frames starting at (inclusive of) the frame containing the given `PhysicalAddress`.
/// 
/// See [`allocate_frames_deferred()`](fn.allocate_frames_deferred.html) for more details. 
pub fn allocate_frames_at(paddr: PhysicalAddress, num_frames: usize) -> Result<Frames<{FrameState::Unmapped}>, &'static str> {
    allocate_frames_deferred(Some(paddr), num_frames)
        .map(|(af, _action)| af)
}


/// Converts the frame allocator from using static memory (a primitive array) to dynamically-allocated memory.
/// 
/// Call this function once heap allocation is available. 
/// Calling this multiple times is unnecessary but harmless, as it will do nothing after the first invocation.
#[doc(hidden)] 
pub fn convert_to_heap_allocated() {
    switch_chunk_allocator_to_heap_structure();
    FREE_GENERAL_FRAMES_LIST.lock().convert_to_heap_allocated();
    FREE_RESERVED_FRAMES_LIST.lock().convert_to_heap_allocated();
    GENERAL_REGIONS.lock().convert_to_heap_allocated();
    RESERVED_REGIONS.lock().convert_to_heap_allocated();
}

/// A debugging function used to dump the full internal state of the frame allocator. 
#[doc(hidden)] 
pub fn dump_frame_allocator_state() {
    debug!("----------------- FREE GENERAL FRAMES ---------------");
    FREE_GENERAL_FRAMES_LIST.lock().iter().for_each(|e| debug!("\t {:?}", e) );
    debug!("-----------------------------------------------------");
    debug!("----------------- FREE RESERVED FRAMES --------------");
    FREE_RESERVED_FRAMES_LIST.lock().iter().for_each(|e| debug!("\t {:?}", e) );
    debug!("-----------------------------------------------------");
    debug!("------------------ GENERAL REGIONS -----------------");
    GENERAL_REGIONS.lock().iter().for_each(|e| debug!("\t {:?}", e) );
    debug!("-----------------------------------------------------");
    debug!("------------------ RESERVED REGIONS -----------------");
    RESERVED_REGIONS.lock().iter().for_each(|e| debug!("\t {:?}", e) );
    debug!("-----------------------------------------------------");
}
