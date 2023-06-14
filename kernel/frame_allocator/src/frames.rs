//! A range of unmapped frames that stores a verified `TrustedChunk`.
//! A `Frames` object is uncloneable and is the only way to access the range of frames it references.
//! 
//! To Do: Merge AllocatedFrames into this typestate as well.

use kernel_config::memory::PAGE_SIZE;
use memory_structs::{FrameRange, Frame, PhysicalAddress};
use range_inclusive::RangeInclusive;
use crate::{MemoryRegionType, AllocatedFrames};
use core::{borrow::Borrow, cmp::Ordering, ops::{Deref, DerefMut}};
use spin::Mutex;
use trusted_chunk::trusted_chunk::*;

static CHUNK_ALLOCATOR: Mutex<TrustedChunkAllocator> = Mutex::new(TrustedChunkAllocator::new());

pub(crate) fn switch_chunk_allocator_to_heap_structure() {
    CHUNK_ALLOCATOR.lock().switch_to_heap_allocated()
        .expect("BUG: Failed to switch the chunk allocator to heap allocated. May have been called twice.");
}

#[derive(PartialEq, Eq)]
pub enum FrameState {
    Unmapped,
}

/// A range of contiguous frames.
/// Owning a `Frames` object gives ownership of the range of frames it references.
/// The `verified_chunk` field is a verified `TrustedChunk` that stores the actual frames,
/// and has the invariant that it does not overlap with any other `TrustedChunk` created by the
/// `CHUNK_ALLOCATOR`.
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
#[derive(Debug, Eq)]
pub struct Frames<const S: FrameState> {
    /// The type of this memory chunk, e.g., whether it's in a free or reserved region.
    typ: MemoryRegionType,
    /// The Frames covered by this chunk, an inclusive range. Equal to the frames in the verified chunk.
    /// Needed because verification fails on a trusted chunk that stores a FrameRange or RangeInclusive<Frame>, 
    /// but succeeds with RangeInclusive<usize>.
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
        
        // assert!(frames.start().number() == verified_chunk.start());
        // assert!(frames.end().number() == verified_chunk.end());

        let f = Frames {
            typ,
            frames,
            verified_chunk
        };
        //warn!("new frames: {:?}", f);
        Ok(f)
    }

    /// Creates a new Chunk from a TrustedChunk and a FrameRange.
    /// Only used within the allocated frames callback function.
    pub(crate) fn from_trusted_chunk(verified_chunk: TrustedChunk, frames: FrameRange, typ: MemoryRegionType) -> Self {
        let f = Frames {
            typ,
            frames,
            verified_chunk
        };
        // assert!(f.frames.start().number() == f.verified_chunk.start());
        // assert!(f.frames.end().number() == f.verified_chunk.end());
        // warn!("from trusted chunk: {:?}", f);
        f
    }

    pub(crate) fn as_allocated_frames(self) -> AllocatedFrames {
        AllocatedFrames {
            frames: self,
        }
    }
}

impl<const S: FrameState> Frames<S> {
    #[allow(dead_code)]
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
        match self.verified_chunk.merge(other_verified_chunk){
            Ok(_) => {
                // use the newly merged TrustedChunk to update the frame range
                self.frames = into_frame_range(&self.verified_chunk.frames());
                core::mem::forget(other);
                // assert!(self.frames.start().number() == self.verified_chunk.start());
                // assert!(self.frames.end().number() == self.verified_chunk.end());
                //warn!("merge: {:?}", self);

                return Ok(());
            },
            Err(other_verified_chunk) => {
                let _ = core::mem::replace(&mut other.verified_chunk, other_verified_chunk);
                // assert!(self.frames.start().number() == self.verified_chunk.start());
                // assert!(self.frames.end().number() == self.verified_chunk.end());
                
                // assert!(other.frames.start().number() == other.verified_chunk.start());
                // assert!(other.frames.end().number() == other.verified_chunk.end());
                return Err(other);
            }
        }
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
                // assert!(self.frames.start().number() == self.verified_chunk.start());
                // assert!(self.frames.end().number() == self.verified_chunk.end());
                return (self, None, None);
            }
        };
        
        let typ = self.typ;
        core::mem::forget(self);

        let c1 = Self {
            typ,
            frames: into_frame_range(&new_allocation.frames()),
            verified_chunk: new_allocation
        };
        let c2 = before.and_then(|vchunk| 
            Some(Self{
                typ,
                frames: into_frame_range(&vchunk.frames()),
                verified_chunk: vchunk
            })
        );
        let c3 = after.and_then(|vchunk| 
            Some(Self{
                typ,
                frames: into_frame_range(&vchunk.frames()),
                verified_chunk: vchunk
            })
        );

        // assert!(c1.frames.start().number() == c1.verified_chunk.start());
        // assert!(c1.frames.end().number() == c1.verified_chunk.end());

        // if let Some(c) = &c2 {
        //     assert!(c.frames.start().number() == c.verified_chunk.start());
        //     assert!(c.frames.end().number() == c.verified_chunk.end());
        // }

        // if let Some(c) = &c3 {
        //     assert!(c.frames.start().number() == c.verified_chunk.start());
        //     assert!(c.frames.end().number() == c.verified_chunk.end());
        // }
        //warn!("split: {:?} {:?} {:?}", c1, c2, c3);

        (c1, c2, c3)
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

        let (first, second) = match verified_chunk.split_at(at_frame.number()){
            Ok((first, second)) => (first, second),
            Err(vchunk) => {
                let _ = core::mem::replace(&mut self.verified_chunk, vchunk);
                // assert!(self.frames.start().number() == self.verified_chunk.start());
                // assert!(self.frames.end().number() == self.verified_chunk.end());
                return Err(self);
            }
        };
        
        core::mem::forget(self);

        let c1 = Self {
            typ,
            frames: into_frame_range(&first.frames()),
            verified_chunk: first
        };
        let c2 = Self {
            typ,
            frames: into_frame_range(&second.frames()),
            verified_chunk: second
        };

        // assert!(c1.frames.start().number() == c1.verified_chunk.start());
        // assert!(c1.frames.end().number() == c1.verified_chunk.end());
        
        // assert!(c2.frames.start().number() == c2.verified_chunk.start());
        // assert!(c2.frames.end().number() == c2.verified_chunk.end());

        //warn!("split at: {:?} {:?}", c1, c2);

        Ok((c1, c2))
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
    let start = into_frame(*frames.start())
        .expect("Verified chunk start was not a valid frame");
    
    let end = into_frame(*frames.end())
        .expect("Verified chunk end was not a valid frame");
    FrameRange::new(start, end)
}

fn into_frame(frame_num: usize) -> Option<Frame> {
    PhysicalAddress::new(frame_num * PAGE_SIZE)
        .and_then(|addr| Some(Frame::containing_address(addr)))
}
