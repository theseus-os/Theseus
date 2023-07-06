//! A range of unmapped frames that stores a verified `TrustedChunk`.
//! A `Frames` object is uncloneable and is the only way to access the range of frames it references.

use memory_structs::{FrameRange, Frame};
use crate::{MemoryRegionType, contains_any, RESERVED_REGIONS, FREE_GENERAL_FRAMES_LIST, FREE_RESERVED_FRAMES_LIST, MIN_FRAME, MAX_FRAME};
use core::{borrow::Borrow, cmp::Ordering, ops::{Deref, DerefMut}, fmt, marker::ConstParamTy};
use static_assertions::assert_not_impl_any;
use log::error;

pub type AllocatedFrames = Frames<{FrameState::Unmapped}>;
pub type AllocatedFrame<'f>  = UnmappedFrame<'f>;


#[derive(PartialEq, Eq, ConstParamTy)]
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
    frames: FrameRange
}

// Frames must not be Cloneable, and it must not expose its inner frames as mutable.
assert_not_impl_any!(Frames<{FrameState::Unmapped}>: DerefMut, Clone);
assert_not_impl_any!(Frames<{FrameState::Mapped}>: DerefMut, Clone);


impl Frames<{FrameState::Unmapped}> {
    /// Creates a new `Frames` object in an unmapped state.
    /// If `frames` is empty, there is no space to store the new `Frames` information pre-heap intialization,
    /// or a `TrustedChunk` already exists which overlaps with the given `frames`, then an error is returned.
    pub(crate) fn new(typ: MemoryRegionType, frames: FrameRange) -> Self {
        // assert!(frames.start().number() == verified_chunk.start());
        // assert!(frames.end().number() == verified_chunk.end());

        Frames {
            typ,
            frames,
        }
        // warn!("NEW FRAMES: {:?}", f);
        // Ok(f)
    }


    /// Consumes the `Frames` in an unmapped state and converts them to `Frames` in a mapped state.
    /// This should only be called once a `MappedPages` has been created from the `Frames`.
    pub fn into_mapped_frames(self) -> Frames<{FrameState::Mapped}> {    
        Frames {
            typ: self.typ,
            frames: self.frames.clone(),
        }
    }

    /// Returns an `UnmappedFrame` if this `Frames<{FrameState::Unmapped}>` object contains only one frame.
    /// I've kept the terminology of allocated frame here to avoid changing code outside of this crate.
    /// ## Panic
    /// Panics if this `UnmappedFrame` contains multiple frames or zero frames.
    pub fn as_allocated_frame(&self) -> UnmappedFrame {
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
pub(crate) fn into_allocated_frames(frames: FrameRange) -> Frames<{FrameState::Unmapped}> {
    let typ = if contains_any(&RESERVED_REGIONS.lock(), &frames) {
        MemoryRegionType::Reserved
    } else {
        MemoryRegionType::Free
    };
    Frames::new(typ, frames)
}

impl<const S: FrameState> Drop for Frames<S> {
    fn drop(&mut self) {
        match S {
            FrameState::Unmapped => {
                if self.size_in_frames() == 0 { return; }
                // trace!("FRAMES DROP {:?}", self);
        
                let unmapped_frames: Frames<{FrameState::Unmapped}> = Frames {
                    typ: self.typ,
                    frames: self.frames.clone(),
                };
        
                let list = if unmapped_frames.typ == MemoryRegionType::Reserved {
                    &FREE_RESERVED_FRAMES_LIST
                } else {
                    &FREE_GENERAL_FRAMES_LIST
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
            range: self.frames.iter(),
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
    /// Returns a tuple of three `Frames`:
    /// 1. The `Frames` containing the requested range of frames starting at `start_frame`.
    /// 2. The range of frames in the `self` that came before the beginning of the requested frame range.
    /// 3. The range of frames in the `self` that came after the end of the requested frame range.
    /// 
    /// If `start_frame` is not contained within `self` or `num_frames` results in an end frame greater than the end of `self`,
    /// then `self` is not changed and we return (self, None, None).
    pub fn split(
        self,
        start_frame: Frame,
        num_frames: usize,
    ) -> (Self, Option<Self>, Option<Self>) {
        if (start_frame < *self.start()) || (start_frame + (num_frames - 1) > *self.end()) || (num_frames <= 0) {
            return (self, None, None);
        }

        let new_allocation = Frames{ frames: FrameRange::new(start_frame, start_frame + (num_frames - 1)), ..self };
        let before = if start_frame == MIN_FRAME || start_frame == *self.start() {
            None
        } else {
            Some(Frames{ frames: FrameRange::new(*self.start(), *new_allocation.start() - 1), ..self })
        };

        let after = if *new_allocation.end() == MAX_FRAME || *new_allocation.end() == *self.end(){
            None
        } else {
            Some(Frames{ frames: FrameRange::new(*new_allocation.end() + 1, *self.end()), ..self })
        };

        core::mem::forget(self);
        (new_allocation, before, after)
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
// To Do: will this be an issue as now this applies to Chunk as well as AllocatedFrames?
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
        write!(f, "Frames({:?}, {:?})", self.typ, self.frames)
    }
}
