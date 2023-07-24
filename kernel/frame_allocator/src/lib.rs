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
//! This allocator only makes one attempt to merge deallocated frames into existing
//! free chunks for de-fragmentation. It does not iteratively merge adjacent chunks in order to
//! maximally combine separate chunks into the biggest single chunk.
//! Instead, free chunks are merged only when they are dropped or when needed to fulfill a specific request.

#![no_std]
#![allow(clippy::blocks_in_if_conditions)]
#![allow(incomplete_features)]
#![feature(adt_const_params)]

extern crate alloc;
#[cfg(test)]
mod test;

mod static_array_rb_tree;
// mod static_array_linked_list;

use core::{borrow::Borrow, cmp::{Ordering, min, max}, ops::{Deref, DerefMut}, fmt};
use intrusive_collections::Bound;
use kernel_config::memory::*;
use log::{error, warn, debug, trace};
use memory_structs::{PhysicalAddress, Frame, FrameRange, MemoryState};
use spin::Mutex;
use static_array_rb_tree::*;
use static_assertions::assert_not_impl_any;

const FRAME_SIZE: usize = PAGE_SIZE;
const MIN_FRAME: Frame = Frame::containing_address(PhysicalAddress::zero());
const MAX_FRAME: Frame = Frame::containing_address(PhysicalAddress::new_canonical(usize::MAX));

// Note: we keep separate lists for "free, general-purpose" areas and "reserved" areas, as it's much faster. 

/// The single, system-wide list of free physical memory frames available for general usage. 
static FREE_GENERAL_FRAMES_LIST: Mutex<StaticArrayRBTree<FreeFrames>> = Mutex::new(StaticArrayRBTree::empty()); 
/// The single, system-wide list of free physical memory frames reserved for specific usage. 
static FREE_RESERVED_FRAMES_LIST: Mutex<StaticArrayRBTree<FreeFrames>> = Mutex::new(StaticArrayRBTree::empty()); 

/// The fixed list of all known regions that are available for general use.
/// This does not indicate whether these regions are currently allocated, 
/// rather just where they exist and which regions are known to this allocator.
static GENERAL_REGIONS: Mutex<StaticArrayRBTree<PhysicalMemoryRegion>> = Mutex::new(StaticArrayRBTree::empty());
/// The fixed list of all known regions that are reserved for specific purposes. 
/// This does not indicate whether these regions are currently allocated, 
/// rather just where they exist and which regions are known to this allocator.
static RESERVED_REGIONS: Mutex<StaticArrayRBTree<PhysicalMemoryRegion>> = Mutex::new(StaticArrayRBTree::empty());


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
/// back into an [`UnmappedFrames`] object.
pub fn init<F, R, P>(
    free_physical_memory_areas: F,
    reserved_physical_memory_areas: R,
) -> Result<fn(FrameRange) -> UnmappedFrames, &'static str> 
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

    let mut free_list: [Option<PhysicalMemoryRegion>; 32] = Default::default();
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


    let mut reserved_list: [Option<PhysicalMemoryRegion>; 32] = Default::default();
    for (i, area) in reserved_physical_memory_areas.into_iter().enumerate() {
        reserved_list[i] = Some(PhysicalMemoryRegion {
            typ: MemoryRegionType::Reserved,
            frames: area.borrow().frames.clone(),
        });
    }

    let mut changed = true;
    while changed {
        let mut temp_reserved_list: [Option<PhysicalMemoryRegion>; 32] = Default::default();
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


    // Finally, one last sanity check -- ensure no two regions overlap. 
    let all_areas = free_list[..free_list_idx].iter().flatten()
        .chain(reserved_list.iter().flatten());
    for (i, elem) in all_areas.clone().enumerate() {
        let next_idx = i + 1;
        for other in all_areas.clone().skip(next_idx) {
            if let Some(overlap) = elem.overlap(other) {
                panic!("BUG: frame allocator free list had overlapping ranges: \n \t {:?} and {:?} overlap at {:?}",
                    elem, other, overlap,
                );
            }
        }
    }

    // Here, since we're sure we now have a list of regions that don't overlap, we can create lists of Frames objects.
    let mut free_list_w_frames: [Option<FreeFrames>; 32] = Default::default();
    let mut reserved_list_w_frames: [Option<FreeFrames>; 32] = Default::default();
    for (i, elem) in reserved_list.iter().flatten().enumerate() {
        reserved_list_w_frames[i] = Some(Frames::new(
            MemoryRegionType::Reserved,
            elem.frames.clone()
        ));
    }

    for (i, elem) in free_list.iter().flatten().enumerate() {
        free_list_w_frames[i] = Some(Frames::new(
            MemoryRegionType::Free,
            elem.frames.clone()
        ));
    }
    *FREE_GENERAL_FRAMES_LIST.lock()  = StaticArrayRBTree::new(free_list_w_frames);
    *FREE_RESERVED_FRAMES_LIST.lock() = StaticArrayRBTree::new(reserved_list_w_frames);
    *GENERAL_REGIONS.lock()           = StaticArrayRBTree::new(free_list);
    *RESERVED_REGIONS.lock()          = StaticArrayRBTree::new(reserved_list);

    Ok(into_unmapped_frames)
}


/// The main logic of the initialization routine 
/// used to populate the list of free frame chunks.
///
/// This function recursively iterates over the given `area` of frames
/// and adds any ranges of frames within that `area` that are not covered by
/// the given list of `reserved_physical_memory_areas`.
fn check_and_add_free_region<P, R>(
    area: &FrameRange,
    free_list: &mut [Option<PhysicalMemoryRegion>; 32],
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
        free_list[*free_list_idx] = Some(PhysicalMemoryRegion {
            typ:  MemoryRegionType::Free,
            frames: new_area,
        });
        *free_list_idx += 1;
    }
}


/// `PhysicalMemoryRegion` represents a range of contiguous frames in physical memory for bookkeeping purposes.
/// It does not give access to the underlying frames.
///
/// # Ordering and Equality
///
/// `PhysicalMemoryRegion` implements the `Ord` trait, and its total ordering is ONLY based on
/// its **starting** `Frame`. This is useful so we can store `PhysicalMemoryRegion`s in a sorted collection.
///
/// Similarly, `PhysicalMemoryRegion` implements equality traits, `Eq` and `PartialEq`,
/// both of which are also based ONLY on the **starting** `Frame` of the `PhysicalMemoryRegion`.
/// Thus, comparing two `PhysicalMemoryRegion`s with the `==` or `!=` operators may not work as expected.
/// since it ignores their actual range of frames.
#[derive(Clone, Debug, Eq)]
pub struct PhysicalMemoryRegion {
    /// The Frames covered by this region, an inclusive range. 
    pub frames: FrameRange,
    /// The type of this memory region, e.g., whether it's in a free or reserved region.
    pub typ: MemoryRegionType,
}
impl PhysicalMemoryRegion {
    pub fn new(frames: FrameRange, typ: MemoryRegionType) -> PhysicalMemoryRegion {
        PhysicalMemoryRegion { frames, typ }
    }

    /// Returns a new `PhysicalMemoryRegion` with an empty range of frames. 
    #[allow(unused)]
    const fn empty() -> PhysicalMemoryRegion {
        PhysicalMemoryRegion {
            typ: MemoryRegionType::Unknown,
            frames: FrameRange::empty(),
        }
    }
}
impl Deref for PhysicalMemoryRegion {
    type Target = FrameRange;
    fn deref(&self) -> &FrameRange {
        &self.frames
    }
}
impl Ord for PhysicalMemoryRegion {
    fn cmp(&self, other: &Self) -> Ordering {
        self.frames.start().cmp(other.frames.start())
    }
}
impl PartialOrd for PhysicalMemoryRegion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl PartialEq for PhysicalMemoryRegion {
    fn eq(&self, other: &Self) -> bool {
        self.frames.start() == other.frames.start()
    }
}
impl Borrow<Frame> for &'_ PhysicalMemoryRegion {
    fn borrow(&self) -> &Frame {
        self.frames.start()
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

/// A range of contiguous frames.
///
/// Owning a `Frames` object implies globally-exclusive ownership of and access to
/// the range of frames it contains.
/// 
/// A `Frames` object can be in one of four states:
/// * `Free`: frames are owned by the frame allocator and have not been allocated for any use.
/// * `Allocated`: frames have been removed from the allocator's free list and are owned elsewhere;
///    they can now be used for mapping purposes.
/// * `Mapped`: frames have been (and are currently) mapped by a range of virtual memory pages.
/// * `Unmapped`: frames have been unmapped and can be returned to the frame allocator.
///
/// The drop behavior for a `Frames` object is based on its state:
/// * `Free`:  the frames will be added back to the frame allocator's free list.
/// * `Allocated`: the frames will be transitioned into the `Free` state.
/// * `Unmapped`: the frames will be transitioned into the `Allocated` state.
/// * `Mapped`: currently, Theseus does not actually drop mapped `Frames`, but rather they are forgotten
///    when they are mapped by virtual pages, and then re-created in the `Unmapped` state
///    after being unmapped from the page tables.
///
/// As such, one can visualize the `Frames` state diagram as such:
/// ```
/// (Free) <---> (Allocated) --> (Mapped) --> (Unmapped) --> (Allocated) <---> (Free)
/// ```
/// 
/// # Ordering and Equality
///
/// `Frames` implements the `Ord` trait, and its total ordering is ONLY based on
/// its **starting** `Frame`. This is useful so we can store `Frames` in a sorted collection.
///
/// Similarly, `Frames` implements equality traits, `Eq` and `PartialEq`,
/// both of which are also based ONLY on the **starting** `Frame` of the `Frames`.
/// Thus, comparing two `Frames` with the `==` or `!=` operators may not work as expected.
/// since it ignores their actual range of frames.
#[derive(Eq)]
pub struct Frames<const S: MemoryState> {
    /// The type of this memory chunk, e.g., whether it's in a free or reserved region.
    typ: MemoryRegionType,
    /// The Frames covered by this chunk, an inclusive range.
    frames: FrameRange
}

/// A type alias for `Frames` in the `Free` state.
pub type FreeFrames = Frames<{MemoryState::Free}>;
/// A type alias for `Frames` in the `Allocated` state.
pub type AllocatedFrames = Frames<{MemoryState::Allocated}>;
/// A type alias for `Frames` in the `Mapped` state.
pub type MappedFrames = Frames<{MemoryState::Mapped}>;
/// A type alias for `Frames` in the `Unmapped` state.
pub type UnmappedFrames = Frames<{MemoryState::Unmapped}>;

// Frames must not be Cloneable, and it must not expose its inner frames as mutable.
assert_not_impl_any!(Frames<{MemoryState::Free}>: DerefMut, Clone);
assert_not_impl_any!(Frames<{MemoryState::Allocated}>: DerefMut, Clone);
assert_not_impl_any!(Frames<{MemoryState::Mapped}>: DerefMut, Clone);
assert_not_impl_any!(Frames<{MemoryState::Unmapped}>: DerefMut, Clone);


impl FreeFrames {
    /// Creates a new `Frames` object in the `Free` state.
    /// The frame allocator is reponsible for making sure no two `Frames` objects overlap.
    pub(crate) fn new(typ: MemoryRegionType, frames: FrameRange) -> Self {
        Frames {
            typ,
            frames,
        }
    }

    /// Consumes this `Frames` in the `Free` state and converts them into the `Allocated` state.

    pub fn into_allocated_frames(self) -> AllocatedFrames {    
        let f = Frames {
            typ: self.typ,
            frames: self.frames.clone(),
        };
        core::mem::forget(self);
        f
    }
}

impl AllocatedFrames {
    /// Consumes this `Frames` in the `Allocated` state and converts them into the `Mapped` state.
    /// This should only be called once a `MappedPages` has been created from the `Frames`.
    pub fn into_mapped_frames(self) -> MappedFrames {    
        let f = Frames {
            typ: self.typ,
            frames: self.frames.clone(),
        };
        core::mem::forget(self);
        f
    }

    /// Returns an `AllocatedFrame` if this `AllocatedFrames` object contains only one frame.
    /// 
    /// ## Panic
    /// Panics if this `AllocatedFrame` contains multiple frames or zero frames.
    pub fn as_allocated_frame(&self) -> AllocatedFrame {
        assert!(self.size_in_frames() == 1);
        AllocatedFrame {
            frame: *self.start(),
            _phantom: core::marker::PhantomData,
        }
    }
}

impl UnmappedFrames {
    /// Consumes the `Frames` in an unmapped state and converts them to `Frames` in an allocated state.
    pub fn into_allocated_frames(self) -> AllocatedFrames {    
        let f = Frames {
            typ: self.typ,
            frames: self.frames.clone(),
        };
        core::mem::forget(self);
        f
    }
}


/// This function is a callback used to convert `UnmappedFramesInfo` into `UnmappedFrames`.
/// `UnmappedFrames` represents frames that have been unmapped from a page that had
/// exclusively mapped to them, indicating that no others pages have been mapped 
/// to those same frames, and thus, they can be safely deallocated.
/// 
/// This exists to break the cyclic dependency cycle between this crate and
/// the `page_table_entry` crate, since `page_table_entry` must depend on types
/// from this crate in order to enforce safety when modifying page table entries.
pub(crate) fn into_unmapped_frames(frames: FrameRange) -> UnmappedFrames {
    let typ = if contains_any(&RESERVED_REGIONS.lock(), &frames) {
        MemoryRegionType::Reserved
    } else {
        MemoryRegionType::Free
    };
    Frames{ typ, frames }
}

impl<const S: MemoryState> Drop for Frames<S> {
    fn drop(&mut self) {
        match S {
            MemoryState::Free => {
                if self.size_in_frames() == 0 { return; }
        
                let free_frames: FreeFrames = Frames {
                    typ: self.typ,
                    frames: self.frames.clone(),
                };
        
                let mut list = if free_frames.typ == MemoryRegionType::Reserved {
                    FREE_RESERVED_FRAMES_LIST.lock()
                } else {
                    FREE_GENERAL_FRAMES_LIST.lock()
                };        
            
                match &mut list.0 {
                    // For early allocations, just add the deallocated chunk to the free pages list.
                    Inner::Array(_) => {
                        if list.insert(free_frames).is_ok() {
                            return;
                        }
                    }
                    
                    // For full-fledged deallocations, use the entry API to efficiently determine if
                    // we can merge the deallocated frames with an existing contiguously-adjacent chunk
                    // or if we need to insert a new chunk.
                    Inner::RBTree(ref mut tree) => {
                        let mut cursor_mut = tree.lower_bound_mut(Bound::Included(free_frames.start()));
                        if let Some(next_frames) = cursor_mut.get_mut() {
                            if *free_frames.end() + 1 == *next_frames.start() {
                                // trace!("Prepending {:?} onto beg of next {:?}", free_frames, next_frames.deref().deref());
                                if next_frames.merge(free_frames).is_ok() {
                                    // trace!("newly merged next chunk: {:?}", next_frames);
                                    return;
                                } else {
                                    panic!("BUG: couldn't merge deallocated chunk into next chunk");
                                }
                            }
                        }
                        if let Some(prev_frames) = cursor_mut.peek_prev().get() {
                            if *prev_frames.end() + 1 == *free_frames.start() {
                                // trace!("Appending {:?} onto end of prev {:?}", free_frames, prev_frames.deref());
                                cursor_mut.move_prev();
                                if let Some(prev_frames) = cursor_mut.get_mut() {
                                    if prev_frames.merge(free_frames).is_ok() {
                                        // trace!("newly merged prev chunk: {:?}", prev_frames);
                                        return;
                                    } else {
                                        panic!("BUG: couldn't merge deallocated chunk into prev chunk");
                                    }
                                }
                            }
                        }

                        // trace!("Inserting new chunk for deallocated {:?} ", free_frames);
                        cursor_mut.insert(Wrapper::new_link(free_frames));
                        return;
                    }
                }
            },
            MemoryState::Allocated => { 
                // trace!("Converting AllocatedFrames to FreeFrames. Drop handler should be called again {:?}", self.frames);
                FreeFrames::new(self.typ, self.frames.clone()); 
            },
            MemoryState::Mapped => panic!("We should never drop a mapped frame! It should be forgotten instead."),
            MemoryState::Unmapped => { AllocatedFrames{ typ: self.typ, frames: self.frames.clone() }; },
        }
    }
}

impl<'f> IntoIterator for &'f AllocatedFrames {
    type IntoIter = AllocatedFramesIter<'f>;
    type Item = AllocatedFrame<'f>;
    fn into_iter(self) -> Self::IntoIter {
        AllocatedFramesIter {
            _owner: self,
            range: self.frames.iter(),
        }
    }
}

/// An iterator over each [`AllocatedFrame`] in a range of [`AllocatedFrames`].
///
/// We must implement our own iterator type here in order to tie the lifetime `'f`
/// of a returned `AllocatedFrame<'f>` type to the lifetime of its containing `AllocatedFrames`.
/// This is because the underlying type of `AllocatedFrames` is a [`FrameRange`],
/// which itself is a [`RangeInclusive`] of [`Frame`]s.
/// Currently, the [`RangeInclusiveIterator`] type creates a clone of the original
/// [`RangeInclusive`] instances rather than borrowing a reference to it.
///
/// [`RangeInclusive`]: range_inclusive::RangeInclusive
pub struct AllocatedFramesIter<'f> {
    _owner: &'f AllocatedFrames,
    range: range_inclusive::RangeInclusiveIterator<Frame>,
}
impl<'f> Iterator for AllocatedFramesIter<'f> {
    type Item = AllocatedFrame<'f>;
    fn next(&mut self) -> Option<Self::Item> {
        self.range.next().map(|frame|
            AllocatedFrame {
                frame, _phantom: core::marker::PhantomData,
            }
        )
    }
}

/// A reference to a single frame within a range of `AllocatedFrames`.
/// 
/// The lifetime of this type is tied to the lifetime of its owning `AllocatedFrames`.
#[derive(Debug)]
pub struct AllocatedFrame<'f> {
    frame: Frame,
    _phantom: core::marker::PhantomData<&'f Frame>,
}
impl<'f> Deref for AllocatedFrame<'f> {
    type Target = Frame;
    fn deref(&self) -> &Self::Target {
        &self.frame
    }
}
assert_not_impl_any!(AllocatedFrame: DerefMut, Clone);

/// The result of splitting a `Frames` object into multiple smaller `Frames` objects.
pub struct SplitFrames<const S: MemoryState>  {
    before_start: Option<Frames<S>>,
    start_to_end:  Frames<S>,
    after_end:    Option<Frames<S>>,
}

impl<const S: MemoryState> Frames<S> {
    #[allow(dead_code)]
    pub(crate) fn frames(&self) -> FrameRange {
        self.frames.clone()
    }

    pub(crate) fn typ(&self) -> MemoryRegionType {
        self.typ
    }

    /// Returns a new `Frames` with an empty range of frames. 
    /// Can be used as a placeholder, but will not permit any real usage.
    pub const fn empty() -> Frames<S> {
        Frames {
            typ: MemoryRegionType::Unknown,
            frames: FrameRange::empty(),
        }
    }

    /// Merges the given `Frames` object `other` into this `Frames` object (`self`).
    /// This is just for convenience and usability purposes, it performs no allocation or remapping.
    ///
    /// The given `other` must be physically contiguous with `self`, i.e., come immediately before or after `self`.
    /// That is, either `self.start == other.end + 1` or `self.end + 1 == other.start` must be true. 
    ///
    /// If either of those conditions are met, `self` is modified and `Ok(())` is returned,
    /// otherwise `Err(other)` is returned.
    pub fn merge(&mut self, other: Self) -> Result<(), Self> {
        if self.is_empty() || other.is_empty() {
            return Err(other);
        }

        if *self.start() == *other.end() + 1 {
            // `other` comes contiguously before `self`
            self.frames = FrameRange::new(*other.start(), *self.end());
        } 
        else if *self.end() + 1 == *other.start() {
            // `self` comes contiguously before `other`
            self.frames = FrameRange::new(*self.start(), *other.end());
        }
        else {
            // non-contiguous
            return Err(other);
        }

        // ensure the now-merged Frames doesn't run its drop handler
        core::mem::forget(other); 
        Ok(())
    }

    /// Splits up the given `Frames` into multiple smaller `Frames`.
    /// 
    /// Returns a `SplitFrames` instance containing three `Frames`:
    /// 2. The range of frames in the `self` that came before the beginning of the requested frame range.
    /// 1. The `Frames` containing the requested range of frames, `frames_to_extract`.
    /// 3. The range of frames in the `self` that came after the end of the requested frame range.
    /// 
    /// If `frames_to_extract` is not contained within `self`, then `self` is not changed and we return it within an Err.
    pub fn split_range(
        self,
        frames_to_extract: FrameRange
    ) -> Result<SplitFrames<S>, Self> {
        
        if !self.contains_range(&frames_to_extract) {
            return Err(self);
        }
        
        let start_frame = *frames_to_extract.start();
        let start_to_end = Frames{ frames: frames_to_extract, ..self };
        
        let before_start = if start_frame == MIN_FRAME || start_frame == *self.start() {
            None
        } else {
            Some(Frames{ frames: FrameRange::new(*self.start(), *start_to_end.start() - 1), ..self })
        };

        let after_end = if *start_to_end.end() == MAX_FRAME || *start_to_end.end() == *self.end(){
            None
        } else {
            Some(Frames{ frames: FrameRange::new(*start_to_end.end() + 1, *self.end()), ..self })
        };

        core::mem::forget(self);
        Ok( SplitFrames{ before_start, start_to_end, after_end} )
    }

    /// Splits this `Frames` into two separate `Frames` objects:
    /// * `[beginning : at_frame - 1]`
    /// * `[at_frame : end]`
    /// 
    /// This function follows the behavior of [`core::slice::split_at()`],
    /// thus, either one of the returned `Frames` objects may be empty. 
    /// * If `at_frame == self.start`, the first returned `Frames` object will be empty.
    /// * If `at_frame == self.end + 1`, the second returned `Frames` object will be empty.
    /// 
    /// Returns an `Err` containing this `Frames` if `at_frame` is otherwise out of bounds, or if `self` was empty.
    /// 
    /// [`core::slice::split_at()`]: https://doc.rust-lang.org/core/primitive.slice.html#method.split_at
    pub fn split_at(self, at_frame: Frame) -> Result<(Self, Self), Self> {
        if self.is_empty() { return Err(self); }

        let end_of_first = at_frame - 1;

        let (first, second) = if at_frame == *self.start() && at_frame <= *self.end() {
            let first  = FrameRange::empty();
            let second = FrameRange::new(at_frame, *self.end());
            (first, second)
        } 
        else if at_frame == (*self.end() + 1) && end_of_first >= *self.start() {
            let first  = FrameRange::new(*self.start(), *self.end()); 
            let second = FrameRange::empty();
            (first, second)
        }
        else if at_frame > *self.start() && end_of_first <= *self.end() {
            let first  = FrameRange::new(*self.start(), end_of_first);
            let second = FrameRange::new(at_frame, *self.end());
            (first, second)
        }
        else {
            return Err(self);
        };

        let typ = self.typ;
        // ensure the original Frames doesn't run its drop handler and free its frames.
        core::mem::forget(self);   
        Ok((
            Frames { typ, frames: first }, 
            Frames { typ, frames: second },
        ))
    }
}

impl<const S: MemoryState> Deref for Frames<S> {
    type Target = FrameRange;
    fn deref(&self) -> &FrameRange {
        &self.frames
    }
}
impl<const S: MemoryState> Ord for Frames<S> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.frames.start().cmp(other.frames.start())
    }
}
impl<const S: MemoryState> PartialOrd for Frames<S> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl<const S: MemoryState> PartialEq for Frames<S> {
    fn eq(&self, other: &Self) -> bool {
        self.frames.start() == other.frames.start()
    }
}
impl<const S: MemoryState> Borrow<Frame> for &'_ Frames<S> {
    fn borrow(&self) -> &Frame {
        self.frames.start()
    }
}

impl<const S: MemoryState> fmt::Debug for Frames<S> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Frames({:?}, {:?})", self.typ, self.frames)
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
    free_list: &'list Mutex<StaticArrayRBTree<FreeFrames>>,
    /// A reference to the list into which we will insert the free "reserved" `Chunk`s.
    reserved_list: &'list Mutex<StaticArrayRBTree<FreeFrames>>,
    /// A free chunk that needs to be added back to the free list.
    free1: FreeFrames,
    /// Another free chunk that needs to be added back to the free list.
    free2: FreeFrames,
}
impl<'list> DeferredAllocAction<'list> {
    fn new<F1, F2>(free1: F1, free2: F2) -> DeferredAllocAction<'list> 
        where F1: Into<Option<FreeFrames>>,
              F2: Into<Option<FreeFrames>>,
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
        let frames1 = core::mem::replace(&mut self.free1, Frames::empty());
        let frames2 = core::mem::replace(&mut self.free2, Frames::empty());
        
        // Insert all of the chunks, both allocated and free ones, into the list. 
        if frames1.size_in_frames() > 0 {
            match frames1.typ() {
                MemoryRegionType::Free     => { self.free_list.lock().insert(frames1).unwrap(); }
                MemoryRegionType::Reserved => { self.reserved_list.lock().insert(frames1).unwrap(); }
                _ => error!("BUG likely: DeferredAllocAction encountered free1 chunk {:?} of a type Unknown", frames1),
            }
        }
        if frames2.size_in_frames() > 0 {
            match frames2.typ() {
                MemoryRegionType::Free     => { self.free_list.lock().insert(frames2).unwrap(); }
                MemoryRegionType::Reserved => { self.reserved_list.lock().insert(frames2).unwrap(); }
                _ => error!("BUG likely: DeferredAllocAction encountered free2 chunk {:?} of a type Unknown", frames2),
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
}
impl From<AllocationError> for &'static str {
    fn from(alloc_err: AllocationError) -> &'static str {
        match alloc_err {
            AllocationError::AddressNotFree(..) => "requested address was in use",
            AllocationError::AddressNotFound(..) => "requested address was outside of this frame allocator's range",
            AllocationError::OutOfAddressSpace(..) => "out of physical address space",
            AllocationError::ContiguousChunkNotFound(..) => "only some of the requested frames were available",
        }
    }
}


/// Searches the given `list` for the chunk that contains the range of frames from
/// `requested_frame` to `requested_frame + num_frames`.
fn find_specific_chunk(
    list: &mut StaticArrayRBTree<FreeFrames>,
    requested_frame: Frame,
    num_frames: usize
) -> Result<(AllocatedFrames, DeferredAllocAction<'static>), AllocationError> {

    // The end frame is an inclusive bound, hence the -1. Parentheses are needed to avoid overflow.
    let requested_end_frame = requested_frame + (num_frames - 1);

    match &mut list.0 {
        Inner::Array(ref mut arr) => {
            for elem in arr.iter_mut() {
                if let Some(chunk) = elem {
                    if requested_frame >= *chunk.start() && requested_end_frame <= *chunk.end() {
                        // Here: `chunk` was big enough and did contain the requested address.
                        return allocate_from_chosen_chunk(FrameRange::new(requested_frame, requested_frame + num_frames - 1), ValueRefMut::Array(elem), None);
                    }
                }
            }
        }
        Inner::RBTree(ref mut tree) => {
            let mut cursor_mut = tree.upper_bound_mut(Bound::Included(&requested_frame));
            if let Some(chunk) = cursor_mut.get().map(|w| w.deref().deref().clone()) {
                if chunk.contains(&requested_frame) {
                    if requested_end_frame <= *chunk.end() {
                        return allocate_from_chosen_chunk(FrameRange::new(requested_frame, requested_frame + num_frames - 1), ValueRefMut::RBTree(cursor_mut), None);
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
                        let next_contiguous_chunk: Option<FreeFrames> = {
                            cursor_mut.move_next();// cursor now points to the next chunk
                            if let Some(next_chunk) = cursor_mut.get().map(|w| w.deref()) {
                                if *chunk.end() + 1 == *next_chunk.start() {
                                    // Here: next chunk was contiguous with the original chunk. 
                                    if requested_end_frame <= *next_chunk.end() {
                                        // trace!("Frame allocator: found suitably-large contiguous next {:?} after initial too-small {:?}", next_chunk, chunk);
                                        let next = cursor_mut.remove().map(|f| f.into_inner());
                                        // after removal, the cursor has been moved to the next chunk, so move it back to the original chunk
                                        cursor_mut.move_prev();
                                        next
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
                        if let Some(next_chunk) = next_contiguous_chunk {
                            // We found a suitable chunk that came contiguously after the initial too-small chunk. 
                            // We would like to merge it into the initial chunk with just the reference (since we have a cursor pointing to it already),
                            // but we can't get a mutable reference to the element the cursor is pointing to.
                            // So both chunks will be removed and then merged. 
                            return allocate_from_chosen_chunk(FrameRange::new(requested_frame, requested_frame + num_frames -1), ValueRefMut::RBTree(cursor_mut), Some(next_chunk));
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
    list: &mut StaticArrayRBTree<FreeFrames>,
    num_frames: usize
) -> Result<(AllocatedFrames, DeferredAllocAction<'static>), AllocationError> {
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
                        return allocate_from_chosen_chunk(FrameRange::new(*chunk.start(), *chunk.start() + num_frames - 1), ValueRefMut::Array(elem), None);
                    }
                }
            }
        }
        Inner::RBTree(ref mut tree) => {
            // Because we allocate new frames by peeling them off from the beginning part of a chunk, 
            // it's MUCH faster to start the search for free frames from higher addresses moving down. 
            // This results in an O(1) allocation time in the general case, until all address ranges are already in use.
            let mut cursor = tree.upper_bound_mut(Bound::<&FreeFrames>::Unbounded);
            while let Some(chunk) = cursor.get().map(|w| w.deref()) {
                if num_frames <= chunk.size_in_frames() && chunk.typ() == MemoryRegionType::Free {
                    return allocate_from_chosen_chunk(FrameRange::new(*chunk.start(), *chunk.start() + num_frames - 1), ValueRefMut::RBTree(cursor), None);
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


/// Removes a `Frames` object from the RBTree. 
/// `frames_ref` is basically a wrapper over the cursor which stores the position of the frames.
fn retrieve_frames_from_ref(mut frames_ref: ValueRefMut<FreeFrames>) -> Option<FreeFrames> {
    // Remove the chosen chunk from the free frame list.
    let removed_val = frames_ref.remove();
    
    match removed_val {
        RemovedValue::Array(c) => c,
        RemovedValue::RBTree(option_frames) => {
            option_frames.map(|c| c.into_inner())
        }
    }
}

/// The final part of the main allocation routine that optionally merges two contiguous chunks and 
/// then splits the resulting chunk into multiple smaller chunks, thereby "allocating" frames from it.
///
/// This function breaks up that chunk into multiple ones and returns an `AllocatedFrames` 
/// from (part of) that chunk that has the same range as `frames_to_allocate`.
fn allocate_from_chosen_chunk(
    frames_to_allocate: FrameRange,
    initial_chunk_ref: ValueRefMut<FreeFrames>,
    next_chunk: Option<FreeFrames>,
) -> Result<(AllocatedFrames, DeferredAllocAction<'static>), AllocationError> {
    // Remove the initial chunk from the free frame list.
    let mut chosen_chunk = retrieve_frames_from_ref(initial_chunk_ref)
        .expect("BUG: Failed to retrieve chunk from free list");
    
    // This should always succeed, since we've already checked the conditions for a merge and split.
    // We should return the chunks back to the list, but a failure at this point implies a bug in the frame allocator.

    if let Some(chunk) = next_chunk {
        chosen_chunk.merge(chunk).expect("BUG: Failed to merge adjacent chunks");
    }

    let SplitFrames{ before_start, start_to_end: new_allocation, after_end } = chosen_chunk.split_range(frames_to_allocate)
        .expect("BUG: Failed to split merged chunk");

    // TODO: Re-use the allocated wrapper if possible, rather than allocate a new one entirely.
    // if let RemovedValue::RBTree(Some(wrapper_adapter)) = _removed_chunk { ... }

    Ok((
        new_allocation.into_allocated_frames(),
        DeferredAllocAction::new(before_start, after_end),
    ))

}


/// Returns `true` if the given list contains *any* of the given `frames`.
fn contains_any(
    list: &StaticArrayRBTree<PhysicalMemoryRegion>,
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

/// Adds the given `frames` to the given `regions_list` and `frames_list` as a chunk of reserved frames. 
/// 
/// Returns the range of **new** frames that were added to the lists, 
/// which will be a subset of the given input `frames`.
///
/// Currently, this function adds no new frames at all if any frames within the given `frames` list
/// overlap any existing regions at all. 
/// TODO: handle partially-overlapping regions by extending existing regions on either end.
fn add_reserved_region_to_lists(
    regions_list: &mut StaticArrayRBTree<PhysicalMemoryRegion>,
    frames_list: &mut StaticArrayRBTree<FreeFrames>,
    frames: FrameRange,
) -> Result<FrameRange, &'static str> {

    // first check the regions list for overlaps and proceed only if there are none.
    if !contains_any(&regions_list, &frames){
        // Check whether the reserved region overlaps any existing regions.
        match &mut frames_list.0 {
            Inner::Array(ref mut arr) => {
                for chunk in arr.iter().flatten() {
                    if let Some(_overlap) = chunk.overlap(&frames) {
                        // trace!("Failed to add reserved region {:?} due to overlap {:?} with existing chunk {:?}",
                        //     frames, _overlap, chunk
                        // );
                        return Err("Failed to add free frames that overlapped with existing frames (array).");
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
                        return Err("Failed to add free frames that overlapped with existing frames (RBTree).");
                    }
                    cursor_mut.move_next();
                }
            }
        }
        
        regions_list.insert(PhysicalMemoryRegion {
                typ: MemoryRegionType::Reserved,
                frames: frames.clone(),
        }).map_err(|_c| "BUG: Failed to insert non-overlapping frames into list.")?;

        frames_list.insert(Frames::new(
            MemoryRegionType::Reserved,
            frames.clone(),
        )).map_err(|_c| "BUG: Failed to insert non-overlapping frames into list.")?;
    } else {
        return Err("Failed to add reserved region that overlapped with existing reserved regions.");
    }
    
    Ok(frames)
}


/// The core frame allocation routine that allocates the given number of physical frames,
/// optionally at the requested starting `PhysicalAddress`.
/// 
/// This simply reserves a range of frames; it does not perform any memory mapping. 
/// Thus, the memory represented by the returned `AllocatedFrames` isn't directly accessible
/// until you map virtual pages to them.
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
            let _new_reserved_frames = add_reserved_region_to_lists(&mut RESERVED_REGIONS.lock(), &mut free_reserved_frames_list, requested_frames)?;
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
        .map(|(af, _action)| af)
        .ok()
}


/// Allocates frames with no constraints on the starting physical address, 
/// with a size given by the number of bytes. 
/// 
/// This function still allocates whole frames by rounding up the number of bytes. 
/// See [`allocate_frames_deferred()`](fn.allocate_frames_deferred.html) for more details. 
pub fn allocate_frames_by_bytes(num_bytes: usize) -> Option<AllocatedFrames> {
    allocate_frames_by_bytes_deferred(None, num_bytes)
        .map(|(af, _action)| af)
        .ok()
}


/// Allocates frames starting at the given `PhysicalAddress` with a size given in number of bytes. 
/// 
/// This function still allocates whole frames by rounding up the number of bytes. 
/// See [`allocate_frames_deferred()`](fn.allocate_frames_deferred.html) for more details. 
pub fn allocate_frames_by_bytes_at(paddr: PhysicalAddress, num_bytes: usize) -> Result<AllocatedFrames, &'static str> {
    allocate_frames_by_bytes_deferred(Some(paddr), num_bytes)
        .map(|(af, _action)| af)
}


/// Allocates the given number of frames starting at (inclusive of) the frame containing the given `PhysicalAddress`.
/// 
/// See [`allocate_frames_deferred()`](fn.allocate_frames_deferred.html) for more details. 
pub fn allocate_frames_at(paddr: PhysicalAddress, num_frames: usize) -> Result<AllocatedFrames, &'static str> {
    allocate_frames_deferred(Some(paddr), num_frames)
        .map(|(af, _action)| af)
}


/// Converts the frame allocator from using static memory (a primitive array) to dynamically-allocated memory.
/// 
/// Call this function once heap allocation is available. 
/// Calling this multiple times is unnecessary but harmless, as it will do nothing after the first invocation.
#[doc(hidden)] 
pub fn convert_frame_allocator_to_heap_based() {
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
