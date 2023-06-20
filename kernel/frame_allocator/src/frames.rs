//! A range of unmapped frames that stores a verified `TrustedChunk`.
//! A `Frames` object is uncloneable and is the only way to access the range of frames it references.

use kernel_config::memory::PAGE_SIZE;
use memory_structs::{FrameRange, Frame, PhysicalAddress};
use range_inclusive::RangeInclusive;
use crate::{MemoryRegionType, contains_any, RESERVED_REGIONS, FREE_GENERAL_FRAMES_LIST, FREE_RESERVED_FRAMES_LIST};
use core::{borrow::Borrow, cmp::Ordering, ops::{Deref, DerefMut}, fmt};
use spin::Mutex;
use trusted_chunk::trusted_chunk::*;

pub type AllocatedFrames = Frames<{FrameState::Unmapped}>;

static CHUNK_ALLOCATOR: Mutex<TrustedChunkAllocator> = Mutex::new(TrustedChunkAllocator::new());

pub(crate) fn switch_chunk_allocator_to_heap_structure() {
    CHUNK_ALLOCATOR.lock().switch_to_heap_allocated()
        .expect("BUG: Failed to switch the chunk allocator to heap allocated. May have been called twice.");
}

#[derive(PartialEq, Eq)]
pub enum FrameState {
    Unmapped,
    Mapped
}

/// A range of contiguous frames.
/// Owning a `Frames` object gives ownership of the range of frames it references.
/// The `verified_chunk` field is a verified `TrustedChunk` that stores the actual frames,
/// and has the invariant that it does not overlap with any other `TrustedChunk` created by the
/// `CHUNK_ALLOCATOR`.
/// 
/// The frames can be in an unmapped or mapped state. In the unmapped state, the frames are not
/// immediately accessible because they're not yet mapped by any virtual memory pages.
/// They are converted into a mapped state once they are used to create a `MappedPages` object.
/// 
/// When a `Frames` object in an unmapped state is dropped, it is deallocated and returned to the free frames list.
/// We expect that `Frames` in a mapped state will never be dropped, but instead will be forgotten.
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

// Frames must not be Cloneable, and it must not expose its inner frames as mutable.
assert_not_impl_any!(Frames<{FrameState::Unmapped}>: DerefMut, Clone);
assert_not_impl_any!(Frames<{FrameState::Mapped}>: DerefMut, Clone);


impl Frames<{FrameState::Unmapped}> {
    /// Creates a new `Frames` object in an unmapped state.
    /// If `frames` is empty, there is no space to store the new `Frames` information pre-heap intialization,
    /// or a `TrustedChunk` already exists which overlaps with the given `frames`, then an error is returned.
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
        // warn!("NEW FRAMES: {:?}", f);
        Ok(f)
    }

    /// Creates a new Chunk from a TrustedChunk and a FrameRange.
    /// It is expected that the range of `verified_chunk` is equal to `frames`.
    /// Only used within the allocated frames callback function.
    pub(crate) fn from_trusted_chunk(verified_chunk: TrustedChunk, frames: FrameRange, typ: MemoryRegionType) -> Self {
        let f = Frames {
            typ,
            frames,
            verified_chunk
        };

        // assert!(f.frames.start().number() == f.verified_chunk.start());
        // assert!(f.frames.end().number() == f.verified_chunk.end());
        // warn!("FROM TRUSTED CHUNK: {:?}", f);
        f
    }

    /// Consumes the `Frames` in an unmapped state and converts them to `Frames` in a mapped state.
    /// This should only be called once a `MappedPages` has been created from the `Frames`.
    pub fn into_mapped_frames(mut self) -> Frames<{FrameState::Mapped}> {
        let typ = self.typ;
        let (frame_range, chunk) = self.replace_with_empty();
        core::mem::forget(self);
        
        Frames {
            typ: typ,
            frames: frame_range,
            verified_chunk: chunk
        }
    }

    /// Returns an `UnmappedFrame` if this `Frames<{FrameState::Unmapped}>` object contains only one frame.
    /// 
    /// ## Panic
    /// Panics if this `AllocatedFrame` contains multiple frames or zero frames.
    pub fn as_unmapped_frame(&self) -> UnmappedFrame {
        assert!(self.size_in_frames() == 1);
        UnmappedFrame {
            frame: *self.start(),
            _phantom: core::marker::PhantomData,
        }
    }
}


/// This function is a callback used to convert `UnmappedFrames` into `Frames<{FrameState::Unmapped}>`.
/// `UnmappedFrames` represents frames that have been unmapped from a page that had
/// exclusively mapped them, indicating that no others pages have been mapped 
/// to those same frames, and thus, they can be safely deallocated.
/// 
/// This exists to break the cyclic dependency cycle between this crate and
/// the `page_table_entry` crate, since `page_table_entry` must depend on types
/// from this crate in order to enforce safety when modifying page table entries.
pub(crate) fn into_allocated_frames(tc: TrustedChunk, frames: FrameRange) -> Frames<{FrameState::Unmapped}> {
    let typ = if contains_any(&RESERVED_REGIONS.lock(), &frames) {
        MemoryRegionType::Reserved
    } else {
        MemoryRegionType::Free
    };
    Frames::from_trusted_chunk(tc, frames, typ)
}

impl<const S: FrameState> Drop for Frames<S> {
    fn drop(&mut self) {
        match S {
            FrameState::Unmapped => {
                if self.size_in_frames() == 0 { return; }
                // trace!("FRAMES DROP {:?}", self);
        
                let (frames, verified_chunk) = self.replace_with_empty();
                let unmapped_frames: Frames<{FrameState::Unmapped}> = Frames {
                    typ: self.typ,
                    frames,
                    verified_chunk,
                };
        
                // Should we remove these lines since we store the typ in Frames?
                let (list, _typ) = if contains_any(&RESERVED_REGIONS.lock(), &unmapped_frames) {
                    (&FREE_RESERVED_FRAMES_LIST, MemoryRegionType::Reserved)
                } else {
                    (&FREE_GENERAL_FRAMES_LIST, MemoryRegionType::Free)
                };
        
                // Simply add the newly-deallocated chunk to the free frames list.
                let mut locked_list = list.lock();
                let res = locked_list.insert(unmapped_frames);
                match res {
                    Ok(_inserted_free_chunk) => (),
                    Err(c) => error!("BUG: couldn't insert deallocated chunk {:?} into free frame list", c),
                }
                
                // Here, we could optionally use above `_inserted_free_chunk` to merge the adjacent (contiguous) chunks
                // before or after the newly-inserted free chunk. 
                // However, there's no *need* to do so until we actually run out of address space or until 
                // a requested address is in a chunk that needs to be merged.
                // Thus, for performance, we save that for those future situations.
            }
            FrameState::Mapped => panic!("We should never drop a mapped frame! It should be forgotten instead."),
        }
    }
}

impl<'f> IntoIterator for &'f Frames<{FrameState::Unmapped}> {
    type IntoIter = UnmappedFramesIter<'f>;
    type Item = UnmappedFrame<'f>;
    fn into_iter(self) -> Self::IntoIter {
        UnmappedFramesIter {
            _owner: self,
            range: self.frames.clone().into_iter(),
        }
    }
}

/// An iterator over each [`UnmappedFrame`] in a range of [`Frames<{FrameState::Unmapped}>`].
/// 
/// To Do: Description is no longer valid, since we have an iterator for RangeInclusive now.
/// but I still think it's useful to have a `Frames<{FrameState::Unmapped}>` iterator that ties the lifetime
/// of the `UnmappedFrame` to the original object.
/// 
/// We must implement our own iterator type here in order to tie the lifetime `'f`
/// of a returned `UnmappedFrame<'f>` type to the lifetime of its containing `Frames<{FrameState::Unmapped}>`.
/// This is because the underlying type of `Frames<{FrameState::Unmapped}>` is a [`FrameRange`],
/// which itself is a [`core::ops::RangeInclusive`] of [`Frame`]s, and unfortunately the
/// `RangeInclusive` type doesn't implement an immutable iterator.
/// 
/// Iterating through a `RangeInclusive` actually modifies its own internal range,
/// so we must avoid doing that because it would break the semantics of a `FrameRange`.
/// In fact, this is why [`FrameRange`] only implements `IntoIterator` but
/// does not implement [`Iterator`] itself.
pub struct UnmappedFramesIter<'f> {
    _owner: &'f Frames<{FrameState::Unmapped}>,
    range: range_inclusive::RangeInclusiveIterator<Frame>,
}
impl<'f> Iterator for UnmappedFramesIter<'f> {
    type Item = UnmappedFrame<'f>;
    fn next(&mut self) -> Option<Self::Item> {
        self.range.next().map(|frame|
            UnmappedFrame {
                frame, _phantom: core::marker::PhantomData,
            }
        )
    }
}

/// A reference to a single frame within a range of `Frames<{FrameState::Unmapped}>`.
/// 
/// The lifetime of this type is tied to the lifetime of its owning `Frames<{FrameState::Unmapped}>`.
#[derive(Debug)]
pub struct UnmappedFrame<'f> {
    frame: Frame,
    _phantom: core::marker::PhantomData<&'f Frame>,
}
impl<'f> Deref for UnmappedFrame<'f> {
    type Target = Frame;
    fn deref(&self) -> &Self::Target {
        &self.frame
    }
}
assert_not_impl_any!(UnmappedFrame: DerefMut, Clone);


impl<const S: FrameState> Frames<S> {
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
            verified_chunk: TrustedChunk::empty()
        }
    }

    /// Returns the `frames` and `verified_chunk` fields of this `Frames` object,
    /// and replaces them with an empty range of frames and an empty `TrustedChunk`.
    /// It's a convenience function to make sure these two fields are always changed together.
    fn replace_with_empty(&mut self) -> (FrameRange, TrustedChunk) {
        let chunk = core::mem::replace(&mut self.verified_chunk, TrustedChunk::empty());
        let frame_range = core::mem::replace(&mut self.frames, FrameRange::empty());
        (frame_range, chunk)
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
        // To Do: Check if we actually need this or does the verified merge function take care of this condition
        if self.is_empty() || other.is_empty() {
            return Err(other);
        }

        // take out the TrustedChunk from other
        let (other_frame_range, other_verified_chunk) = other.replace_with_empty();
        
        // merged the other TrustedChunk with self
        // failure here means that the chunks cannot be merged
        match self.verified_chunk.merge(other_verified_chunk){
            Ok(_) => {
                // use the newly merged TrustedChunk to update the frame range
                self.frames = into_frame_range(&self.verified_chunk.frames());
                core::mem::forget(other);
                // assert!(self.frames.start().number() == self.verified_chunk.start());
                // assert!(self.frames.end().number() == self.verified_chunk.end());
                // warn!("merge: {:?}", self);
                Ok(())
            },
            Err(other_verified_chunk) => {
                other.frames = other_frame_range;
                other.verified_chunk = other_verified_chunk;

                // assert!(self.frames.start().number() == self.verified_chunk.start());
                // assert!(self.frames.end().number() == self.verified_chunk.end());
                
                // assert!(other.frames.start().number() == other.verified_chunk.start());
                // assert!(other.frames.end().number() == other.verified_chunk.end());
                Err(other)
            }
        }
    }

    /// Splits up the given `Frames` into multiple smaller `Frames`.
    /// 
    /// Returns a tuple of three `Frames`:
    /// 1. The `Frames` containing the requested range of frames starting at `start_frame`.
    /// 2. The range of frames in the `self` that came before the beginning of the requested frame range.
    /// 3. The range of frames in the `self` that came after the end of the requested frame range.
    /// 
    /// If `start_frame` is not contained within `self` or `num_frames` results in an end frame greater than the end of `self`,
    /// then `self` is not changed and we return (self, None, None).
    pub fn split(
        mut self,
        start_frame: Frame,
        num_frames: usize,
    ) -> (Self, Option<Self>, Option<Self>) {
        if self.is_empty() {
            return (self, None, None);
        }

        // take out the TrustedChunk
        let (frame_range, verified_chunk) = self.replace_with_empty();

        let (before, new_allocation, after) = match verified_chunk.split(start_frame.number(), num_frames) {
            Ok(x) => x,
            Err(vchunk) => {
                self.frames = frame_range;
                self.verified_chunk = vchunk;

                // assert!(self.frames.start().number() == self.verified_chunk.start());
                // assert!(self.frames.end().number() == self.verified_chunk.end());
                return (self, None, None);
            }
        };

        let c1 = Self {
            typ: self.typ,
            frames: into_frame_range(&new_allocation.frames()),
            verified_chunk: new_allocation
        };
        let c2 = before.map(|vchunk| 
            Self{
                typ: self.typ,
                frames: into_frame_range(&vchunk.frames()),
                verified_chunk: vchunk
            }
        );
        let c3 = after.map(|vchunk| 
            Self{
                typ: self.typ,
                frames: into_frame_range(&vchunk.frames()),
                verified_chunk: vchunk
            }
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
        // warn!("split: {:?} {:?} {:?}", c1, c2, c3);
        core::mem::forget(self);

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

        // take out the TrustedChunk
        let (frame_range, verified_chunk) = self.replace_with_empty();

        let (first, second) = match verified_chunk.split_at(at_frame.number()){
            Ok((first, second)) => (first, second),
            Err(vchunk) => {
                self.frames = frame_range;
                self.verified_chunk = vchunk;

                // assert!(self.frames.start().number() == self.verified_chunk.start());
                // assert!(self.frames.end().number() == self.verified_chunk.end());
                return Err(self);
            }
        };

        let c1 = Self {
            typ: self.typ,
            frames: into_frame_range(&first.frames()),
            verified_chunk: first
        };
        let c2 = Self {
            typ: self.typ,
            frames: into_frame_range(&second.frames()),
            verified_chunk: second
        };

        // assert!(c1.frames.start().number() == c1.verified_chunk.start());
        // assert!(c1.frames.end().number() == c1.verified_chunk.end());
        
        // assert!(c2.frames.start().number() == c2.verified_chunk.start());
        // assert!(c2.frames.end().number() == c2.verified_chunk.end());

        // warn!("split at: {:?} {:?}", c1, c2);
        core::mem::forget(self);

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
// To Do: will this be an issue as now this applies to Chunk as well as AllocatedFrames
#[cfg(not(test))]
impl<const S: FrameState> PartialEq for Frames<S> {
    fn eq(&self, other: &Self) -> bool {
        self.frames.start() == other.frames.start()
    }
}
#[cfg(test)]
impl<const S: FrameState> PartialEq for Frames<S> {
    fn eq(&self, other: &Self) -> bool {
        self.frames == other.frames
    }
}
impl<const S: FrameState> Borrow<Frame> for &'_ Frames<S> {
    fn borrow(&self) -> &Frame {
        self.frames.start()
    }
}
impl<const S: FrameState> fmt::Debug for Frames<S> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Frames({:?}, {:?}, TrustedChunk:{{ start: {:#X}, end: {:#X} }})", self.typ, self.frames, 
        self.verified_chunk.frames().start() * PAGE_SIZE, self.verified_chunk.frames().end()* PAGE_SIZE)
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
        .map(Frame::containing_address)
}
