use crate::{Chunk, MemoryRegionType, contains_any, FREE_GENERAL_FRAMES_LIST, FREE_RESERVED_FRAMES_LIST, RESERVED_REGIONS};
use memory_structs::{FrameRange, Frame};
use core::{fmt, ops::{Deref, DerefMut}, marker::PhantomData};
use trusted_chunk::trusted_chunk::TrustedChunk;
use range_inclusive::RangeInclusiveIterator;

/// Represents a range of allocated physical memory [`Frame`]s; derefs to [`FrameRange`].
/// 
/// These frames are not immediately accessible because they're not yet mapped
/// by any virtual memory pages.
/// You must do that separately in order to create a `MappedPages` type,
/// which can then be used to access the contents of these frames.
/// 
/// This object represents ownership of the range of allocated physical frames;
/// if this object falls out of scope, its allocated frames will be auto-deallocated upon drop. 
pub struct AllocatedFrames {
    pub(crate) frames: Chunk,
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
            frames: Chunk::empty()
        }
    }

    /// Merges the given `AllocatedFrames` object `other` into this `AllocatedFrames` object (`self`).
    /// This is just for convenience and usability purposes, it performs no allocation or remapping.
    ///
    /// The given `other` must be physically contiguous with `self`, i.e., come immediately before or after `self`.
    /// That is, either `self.start == other.end + 1` or `self.end + 1 == other.start` must be true. 
    ///
    /// If either of those conditions are met, `self` is modified and `Ok(())` is returned,
    /// otherwise `Err(other)` is returned.
    pub fn merge(&mut self, mut other: AllocatedFrames) -> Result<(), AllocatedFrames> {
        let chunk = core::mem::replace(&mut other.frames, Chunk::empty());
        match self.frames.merge(chunk) {
            Ok(_) => {
                // ensure the now-merged AllocatedFrames doesn't run its drop handler and free its frames.
                // This is not really necessary because it only contains an empty chunk.
                core::mem::forget(other); 
                Ok(())
            },
            Err(other_chunk) => {
                Err(AllocatedFrames{frames: other_chunk})
            }
        }
    }

    /// Splits this `AllocatedFrames` into two separate `AllocatedFrames` objects:
    /// * `[beginning : at_frame - 1]`
    /// * `[at_frame : end]`
    /// 
    /// This function follows the behavior of [`core::slice::split_at()`],
    /// thus, either one of the returned `AllocatedFrames` objects may be empty. 
    /// * If `at_frame == self.start`, the first returned `AllocatedFrames` object will be empty.
    /// * If `at_frame == self.end + 1`, the second returned `AllocatedFrames` object will be empty.
    /// 
    /// Returns an `Err` containing this `AllocatedFrames` if `at_frame` is otherwise out of bounds.
    /// 
    /// [`core::slice::split_at()`]: https://doc.rust-lang.org/core/primitive.slice.html#method.split_at
    pub fn split(mut self, at_frame: Frame) -> Result<(AllocatedFrames, AllocatedFrames), AllocatedFrames> {
         let chunk = core::mem::replace(&mut self.frames, Chunk::empty());
        match chunk.split_at(at_frame) {
            Ok((chunk1, chunk2)) => {
                // ensure the now-merged AllocatedFrames doesn't run its drop handler and free its frames.
                core::mem::forget(self); 
                Ok((
                    AllocatedFrames{frames: chunk1}, 
                    AllocatedFrames{frames: chunk2}
                ))
            },
            Err(chunk_not_split) => {
                Err(AllocatedFrames{frames: chunk_not_split})
            }
        }
    }

    /// Returns an `AllocatedFrame` if this `AllocatedFrames` object contains only one frame.
    /// 
    /// ## Panic
    /// Panics if this `AllocatedFrame` contains multiple frames or zero frames.
    pub fn as_allocated_frame(&self) -> AllocatedFrame {
        assert!(self.size_in_frames() == 1);
        AllocatedFrame {
            frame: *self.start(),
            _phantom: PhantomData,
        }
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
pub(crate) fn into_allocated_frames(tc: TrustedChunk, frames: FrameRange) -> AllocatedFrames {
    let typ = if contains_any(&RESERVED_REGIONS.lock(), &frames) {
        MemoryRegionType::Reserved
    } else {
        MemoryRegionType::Free
    };
    AllocatedFrames { frames: Chunk::from_trusted_chunk(tc, frames, typ) }
}

impl Drop for AllocatedFrames {
    fn drop(&mut self) {
        if self.size_in_frames() == 0 { return; }

        let (list, _typ) = if contains_any(&RESERVED_REGIONS.lock(), &self.frames) {
            (&FREE_RESERVED_FRAMES_LIST, MemoryRegionType::Reserved)
        } else {
            (&FREE_GENERAL_FRAMES_LIST, MemoryRegionType::Free)
        };
        // trace!("frame_allocator: deallocating {:?}, typ {:?}", self, typ);

        // Simply add the newly-deallocated chunk to the free frames list.
        let mut locked_list = list.lock();
        let res = locked_list.insert(core::mem::replace(&mut self.frames, Chunk::empty()));
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
}

impl<'f> IntoIterator for &'f AllocatedFrames {
    type IntoIter = AllocatedFramesIter<'f>;
    type Item = AllocatedFrame<'f>;
    fn into_iter(self) -> Self::IntoIter {
        AllocatedFramesIter {
            _owner: self,
            range: self.frames.clone().into_iter(),
        }
    }
}

/// An iterator over each [`AllocatedFrame`] in a range of [`AllocatedFrames`].
/// 
/// We must implement our own iterator type here in order to tie the lifetime `'f`
/// of a returned `AllocatedFrame<'f>` type to the lifetime of its containing `AllocatedFrames`.
/// This is because the underlying type of `AllocatedFrames` is a [`FrameRange`],
/// which itself is a [`core::ops::RangeInclusive`] of [`Frame`]s, and unfortunately the
/// `RangeInclusive` type doesn't implement an immutable iterator.
/// 
/// Iterating through a `RangeInclusive` actually modifies its own internal range,
/// so we must avoid doing that because it would break the semantics of a `FrameRange`.
/// In fact, this is why [`FrameRange`] only implements `IntoIterator` but
/// does not implement [`Iterator`] itself.
pub struct AllocatedFramesIter<'f> {
    _owner: &'f AllocatedFrames,
    range: RangeInclusiveIterator<Frame>,
}
impl<'f> Iterator for AllocatedFramesIter<'f> {
    type Item = AllocatedFrame<'f>;
    fn next(&mut self) -> Option<Self::Item> {
        self.range.next().map(|frame|
            AllocatedFrame {
                frame, _phantom: PhantomData,
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
    _phantom: PhantomData<&'f Frame>,
}
impl<'f> Deref for AllocatedFrame<'f> {
    type Target = Frame;
    fn deref(&self) -> &Self::Target {
        &self.frame
    }
}
assert_not_impl_any!(AllocatedFrame: DerefMut, Clone);
