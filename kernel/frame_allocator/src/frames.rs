use kernel_config::memory::PAGE_SIZE;
use memory_structs::{FrameRange, Frame, PhysicalAddress};
use range_inclusive::RangeInclusive;
use crate::{MemoryRegionType, MIN_FRAME, MAX_FRAME, RESERVED_REGIONS, FREE_GENERAL_FRAMES_LIST, FREE_RESERVED_FRAMES_LIST, contains_any};
use core::{borrow::Borrow, cmp::{Ordering, min, max}, fmt, ops::{Deref, DerefMut}};
use spin::{Once, Mutex};
use trusted_chunk::{
    trusted_chunk::*,
    linked_list::List,
    static_array::StaticArray,
};
use range_inclusive::RangeInclusiveIterator;
use core::marker::PhantomData;
use crate::allocated_frames::*;

static CHUNK_ALLOCATOR: Mutex<TrustedChunkAllocator> = Mutex::new(TrustedChunkAllocator::new());

pub(crate) fn switch_chunk_allocator_to_heap_structure() {
    CHUNK_ALLOCATOR.lock().switch_to_heap_allocated()
        .expect("BUG: Failed to switch the chunk allocator to heap allocated. May have been called twice.");
}

// pub(crate) type AllocatedFrames = Frames<{FrameState::Unmapped}>;
// pub(crate) type MappedFrames = Frames<{FrameState::Mapped}>;


#[derive(PartialEq, Eq)]
pub enum FrameState {
    Unmapped,
}

#[derive(Debug, Eq)]
pub struct Frames<const S: FrameState> {
    /// The type of this memory chunk, e.g., whether it's in a free or reserved region.
    typ: MemoryRegionType,
    /// The Frames covered by this chunk, an inclusive range. 
    frames: FrameRange,
    /// The actual verified chunk
    verified_chunk: TrustedChunk
}

assert_not_impl_any!(Frames<{FrameState::Unmapped}>: DerefMut, Clone);

impl Frames<{FrameState::Unmapped}> {
    pub(crate) fn new(typ: MemoryRegionType, frames: FrameRange) -> Result<Self, &'static str> {
        let verified_chunk = CHUNK_ALLOCATOR.lock().create_chunk(frames.to_range_inclusive())
            .map(|(chunk, _)| chunk)
            .map_err(|chunk_error|{
                match chunk_error {
                    ChunkCreationError::Overlap(_idx) => "Failed to create a verified chunk due to an overlap",
                    ChunkCreationError::NoSpace => "Before the heap is initialized, requested more chunks than there is space for (64)",
                    ChunkCreationError::InvalidRange => "Could not create a chunk for an empty range, use the empty() function"
                }
            })?;

        Ok(Frames {
            typ,
            frames,
            verified_chunk
        })
    }

    /// Creates a new Chunk from a TrustedChunk and a FrameRange.
    /// Only used within the allocated frames callback function.
    pub(crate) fn from_trusted_chunk(verified_chunk: TrustedChunk, frames: FrameRange, typ: MemoryRegionType) -> Self {
        Frames {
            typ,
            frames,
            verified_chunk
        }
    }

    pub(crate) fn as_allocated_frames(self) -> AllocatedFrames {
        AllocatedFrames {
            frames: self,
        }
    }
    // /// Returns an `AllocatedFrame` if this `AllocatedFrames` object contains only one frame.
    // /// 
    // /// ## Panic
    // /// Panics if this `AllocatedFrame` contains multiple frames or zero frames.
    // pub fn as_allocated_frame(&self) -> AllocatedFrame {
    //     assert!(self.size_in_frames() == 1);
    //     AllocatedFrame {
    //         frame: *self.start(),
    //         _phantom: PhantomData,
    //     }
    // }
}

// impl<const S: FrameState> Drop for Frames<S> {
//     fn drop(&mut self) {
//         if self.size_in_frames() == 0 { return; }

//         let unmapped_frames: Frames<{FrameState::Unmapped}> = Frames {
//             typ: self.typ,
//             frames: self.frames.clone(),
//             verified_chunk: core::mem::replace(&mut self.verified_chunk, TrustedChunk::empty())
//         };

//         let (list, _typ) = if contains_any(&RESERVED_REGIONS.lock(), &self.frames) {
//             (&FREE_RESERVED_FRAMES_LIST, MemoryRegionType::Reserved)
//         } else {
//             (&FREE_GENERAL_FRAMES_LIST, MemoryRegionType::Free)
//         };
//         // trace!("frame_allocator: deallocating {:?}, typ {:?}", self, typ);

//         // Simply add the newly-deallocated chunk to the free frames list.
//         let mut locked_list = list.lock();
//         let res = locked_list.insert(unmapped_frames);
//         match res {
//             Ok(_inserted_free_chunk) => (),
//             Err(c) => error!("BUG: couldn't insert deallocated chunk {:?} into free frame list", c),
//         }
        
//         // Here, we could optionally use above `_inserted_free_chunk` to merge the adjacent (contiguous) chunks
//         // before or after the newly-inserted free chunk. 
//         // However, there's no *need* to do so until we actually run out of address space or until 
//         // a requested address is in a chunk that needs to be merged.
//         // Thus, for performance, we save that for those future situations.
//     }
// }

// impl<'f> IntoIterator for &'f Frames<{FrameState::Unmapped}> {
//     type IntoIter = AllocatedFramesIter<'f>;
//     type Item = AllocatedFrame<'f>;
//     fn into_iter(self) -> Self::IntoIter {
//         AllocatedFramesIter {
//             _owner: self,
//             range: self.frames.clone().into_iter(),
//         }
//     }
// }

// /// An iterator over each [`AllocatedFrame`] in a range of [`Frames`].
// /// 
// /// We must implement our own iterator type here in order to tie the lifetime `'f`
// /// of a returned `AllocatedFrame<'f>` type to the lifetime of its containing `Frames`.
// /// This is because the underlying type of `Frames` is a [`FrameRange`],
// /// which itself is a [`core::ops::RangeInclusive`] of [`Frame`]s, and unfortunately the
// /// `RangeInclusive` type doesn't implement an immutable iterator.
// /// 
// /// Iterating through a `RangeInclusive` actually modifies its own internal range,
// /// so we must avoid doing that because it would break the semantics of a `FrameRange`.
// /// In fact, this is why [`FrameRange`] only implements `IntoIterator` but
// /// does not implement [`Iterator`] itself.
// pub struct AllocatedFramesIter<'f> {
//     _owner: &'f Frames<{FrameState::Unmapped}>,
//     range: RangeInclusiveIterator<Frame>,
// }
// impl<'f> Iterator for AllocatedFramesIter<'f> {
//     type Item = AllocatedFrame<'f>;
//     fn next(&mut self) -> Option<Self::Item> {
//         self.range.next().map(|frame|
//             AllocatedFrame {
//                 frame, _phantom: PhantomData,
//             }
//         )
//     }
// }

// /// A reference to a single frame within a range of `Frames`.
// /// 
// /// The lifetime of this type is tied to the lifetime of its owning `Frames`.
// #[derive(Debug)]
// pub struct AllocatedFrame<'f> {
//     frame: Frame,
//     _phantom: PhantomData<&'f Frame>,
// }
// impl<'f> Deref for AllocatedFrame<'f> {
//     type Target = Frame;
//     fn deref(&self) -> &Self::Target {
//         &self.frame
//     }
// }
// assert_not_impl_any!(AllocatedFrame: DerefMut, Clone);


impl<const S: FrameState> Frames<S> {
    pub(crate) fn frames(&self) -> FrameRange {
        self.frames.clone()
    }

    pub(crate) fn typ(&self) -> MemoryRegionType {
        self.typ
    }

    /// Returns a new `Frames` with an empty range of frames. 
    pub const fn empty() -> Frames<S> {
        Frames {
            typ: MemoryRegionType::Unknown,
            frames: FrameRange::empty(),
            verified_chunk: TrustedChunk::empty()
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
    pub fn merge(&mut self, mut other: Self) -> Result<(), Self> {
        if self.is_empty() || other.is_empty() {
            return Err(other);
        }

        // take out the TrustedChunk from other
        let other_verified_chunk = core::mem::replace(&mut other.verified_chunk, TrustedChunk::empty());
        
        // merged the other TrustedChunk with self
        // failure here means that the chunks cannot be merged
        self.verified_chunk.merge(other_verified_chunk)
            .map_err(|vchunk| {
                let _ = core::mem::replace(&mut other.verified_chunk, vchunk);
                other
            })?;

        // use the newly merged TrustedChunk to update the frame range
        self.frames = into_frame_range(&self.verified_chunk.frames());

        Ok(())
    }

    /// An inner function that breaks up the given `Frames` into multiple smaller `Frames`.
    /// 
    /// Returns a tuple of three `Frames`:
    /// 1. The `Frames` containing the requested range of frames starting at `start_frame`.
    /// 2. The range of frames in the `self` that came before the beginning of the requested frame range.
    /// 3. The range of frames in the `self` that came after the end of the requested frame range.
    pub fn split(
        mut self,
        start_frame: Frame,
        num_frames: usize,
    ) -> (Self, Option<Self>, Option<Self>) {
        if self.is_empty() {
            return (self, None, None);
        }

        // take out the TrustedChunk
        let verified_chunk = core::mem::replace(&mut self.verified_chunk, TrustedChunk::empty());

        let (before, new_allocation, after) = match verified_chunk.split(start_frame.number(), num_frames) {
            Ok(x) => x,
            Err(vchunk) => {
                let _ = core::mem::replace(&mut self.verified_chunk, vchunk);
                return (self, None, None);
            }
        };

        (Self {
            typ: self.typ,
            frames: into_frame_range(&new_allocation.frames()),
            verified_chunk: new_allocation
        },
        before.and_then(|vchunk| 
            Some(Self{
                typ: self.typ,
                frames: into_frame_range(&vchunk.frames()),
                verified_chunk: vchunk
            })
        ), 
        after.and_then(|vchunk| 
            Some(Self{
                typ: self.typ,
                frames: into_frame_range(&vchunk.frames()),
                verified_chunk: vchunk
            })
        ))
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
    /// Returns an `Err` containing this `Frames` if `at_frame` is otherwise out of bounds.
    /// 
    /// [`core::slice::split_at()`]: https://doc.rust-lang.org/core/primitive.slice.html#method.split_at
    pub fn split_at(mut self, at_frame: Frame) -> Result<(Self, Self), Self> {
        if self.is_empty() {
            return Err(self);
        }
        let typ = self.typ;

        // take out the TrustedChunk
        let verified_chunk = core::mem::replace(&mut self.verified_chunk, TrustedChunk::empty());

        let (first, second) = verified_chunk.split_at(at_frame.number())
            .map_err(|vchunk| {
                let _ = core::mem::replace(&mut self.verified_chunk, vchunk);
                self
            })?;

        Ok((Self {
            typ,
            frames: into_frame_range(&first.frames()),
            verified_chunk: first
        },
        Self {
            typ,
            frames: into_frame_range(&second.frames()),
            verified_chunk: second
        }))
    }
}

impl<const S: FrameState> Deref for Frames<S> {
    type Target = FrameRange;
    fn deref(&self) -> &FrameRange {
        &self.frames
    }
}
impl<const S: FrameState> Ord for Frames<S> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.frames.start().cmp(other.frames.start())
    }
}
impl<const S: FrameState> PartialOrd for Frames<S> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl<const S: FrameState> PartialEq for Frames<S> {
    fn eq(&self, other: &Self) -> bool {
        self.frames.start() == other.frames.start()
    }
}
impl<const S: FrameState> Borrow<Frame> for &'_ Frames<S> {
    fn borrow(&self) -> &Frame {
        self.frames.start()
    }
}


fn into_frame_range(frames: &RangeInclusive<usize>) -> FrameRange {
    let start = FrameNum{ frame: *frames.start() }.into_frame()
        .expect("Verified chunk start was not a valid frame");
    
    let end = FrameNum{ frame: *frames.end() }.into_frame()
        .expect("Verified chunk end was not a valid frame");
    
    FrameRange::new(start, end)
}

struct FrameNum {
    frame: usize
}

impl FrameNum {
    fn into_frame(&self) -> Option<Frame> {
        PhysicalAddress::new(self.frame * PAGE_SIZE)
            .and_then(|addr| Some(Frame::containing_address(addr)))
    }
}

/// This function is a callback used to convert `UnmappedFrames` into `AllocatedFrames`.
/// `UnmappedFrames` represents frames that have been unmapped from a page that had
/// exclusively mapped them, indicating that no others pages have been mapped 
/// to those same frames, and thus, they can be safely deallocated.
/// 
/// This exists to break the cyclic dependency cycle between this crate and
/// the `page_table_entry` crate, since `page_table_entry` must depend on types
/// from this crate in order to enforce safety when modifying page table entries.
pub(crate) fn into_frames_unmapped_state(tc: TrustedChunk, frames: FrameRange) -> Frames<{FrameState::Unmapped}> {
    let typ = if contains_any(&RESERVED_REGIONS.lock(), &frames) {
        MemoryRegionType::Reserved
    } else {
        MemoryRegionType::Free
    };
    Frames { typ, frames, verified_chunk: tc }
}