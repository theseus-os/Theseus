use super::chunk_range_wrapper::{
    ChunkRangeWrapper, ChunkRangeWrapperPhysical, ChunkRangeWrapperVirtual, MemoryRegionType,
};
use crate::{
    allocator::{physical, virt},
    Chunk, ChunkRange, MemoryType, Physical, Virtual,
};
use core::ops::{Deref, DerefMut};
use log::error;

#[derive(Debug)]
pub struct AllocatedChunks<T>
where
    T: MemoryType,
{
    /// Drop can't be specialized (i.e. different drop impls for `AllocatedChunks<Virtual>` vs.
    /// `AllocatedChunks<Physical>`). This inner type has a custom drop implementation letting
    /// us work around the limitation.
    inner: T::AllocatedChunksInner,
}

impl<T> AllocatedChunks<T>
where
    T: MemoryType,
{
    pub const fn new(chunks: ChunkRange<T>) -> Self
    where
        T::AllocatedChunksInner: ~const AllocatedChunksInner,
    {
        Self {
            inner: T::AllocatedChunksInner::new(chunks),
        }
    }

    pub const fn empty() -> Self
    where
        <T as MemoryType>::AllocatedChunksInner: ~const AllocatedChunksInner,
    {
        Self::new(ChunkRange::empty())
    }

    pub fn merge(&mut self, other: Self) -> Result<(), Self> {
        // TODO: Different to AllocatedFrames impl.

        if *self.start() == *other.end() + 1 {
            // `other` comes contiguously before `self`
            *self.inner = ChunkRange::new(*other.start(), *self.end());
        } else if *self.end() + 1 == *other.start() {
            // `self` comes contiguously before `other`
            *self.inner = ChunkRange::new(*self.start(), *other.end());
        } else {
            // non-contiguous
            return Err(other);
        }

        // Ensure the now-merged AllocatedFrames doesn't run its drop handler and free its frames.
        core::mem::forget(other);
        Ok(())
    }

    pub fn split(self, at_frame: Chunk<T>) -> Result<(Self, Self), Self> {
        let end_of_first = at_frame - 1;

        let (first, second) = if at_frame == *self.start() && at_frame <= *self.end() {
            let first = ChunkRange::empty();
            let second = ChunkRange::new(at_frame, *self.end());
            (first, second)
        } else if at_frame == (*self.end() + 1) && end_of_first >= *self.start() {
            let first = ChunkRange::new(*self.start(), *self.end());
            let second = ChunkRange::empty();
            (first, second)
        } else if at_frame > *self.start() && end_of_first <= *self.end() {
            let first = ChunkRange::new(*self.start(), end_of_first);
            let second = ChunkRange::new(at_frame, *self.end());
            (first, second)
        } else {
            return Err(self);
        };

        // ensure the original AllocatedFrames doesn't run its drop handler and free its frames.
        core::mem::forget(self);
        Ok((Self::new(first), Self::new(second)))
    }
}

impl<T> const Deref for AllocatedChunks<T>
where
    T: MemoryType,
{
    type Target = ChunkRange<T>;

    fn deref(&self) -> &Self::Target
    where
        T::AllocatedChunksInner: ~const Deref,
    {
        self.inner.deref()
    }
}

pub trait AllocatedChunksInner:
    Deref<Target = ChunkRange<Self::MemoryType>> + DerefMut<Target = ChunkRange<Self::MemoryType>>
{
    type MemoryType: MemoryType;

    fn new(chunks: ChunkRange<Self::MemoryType>) -> Self;
}

macro_rules! allocated_chunk_inner {
    ($type:ident, $memory_type:ident) => {
        pub struct $type {
            chunks: ChunkRange<$memory_type>,
        }

        impl const AllocatedChunksInner for $type {
            type MemoryType = $memory_type;

            fn new(chunks: ChunkRange<Self::MemoryType>) -> Self {
                Self { chunks }
            }
        }

        impl const Deref for $type {
            type Target = ChunkRange<<Self as AllocatedChunksInner>::MemoryType>;

            fn deref(&self) -> &Self::Target {
                &self.chunks
            }
        }

        // `DerefMut` is implemented for inner but it is NOT implemented for `AllocatedChunks<T>`.
        impl DerefMut for $type {
            fn deref_mut(&mut self) -> &mut Self::Target {
                &mut self.chunks
            }
        }
    };
}

allocated_chunk_inner!(AllocatedChunksVirtual, Virtual);

impl Drop for AllocatedChunksVirtual {
    fn drop(&mut self) {
        if self.size_in_chunks() == 0 {
            return;
        }

        // Simply add the newly-deallocated chunk to the free pages list.
        let mut locked_list = virt::FREE_PAGE_LIST.lock();
        let res = locked_list.insert(ChunkRangeWrapper {
            chunks: self.chunks.clone(),
            inner: ChunkRangeWrapperVirtual,
        });
        match res {
            Ok(_inserted_free_chunk) => (),
            Err(c) => error!(
                "BUG: couldn't insert deallocated chunk {:?} into free page list",
                c
            ),
        }

        // Here, we could optionally use above `_inserted_free_chunk` to merge the adjacent (contiguous) chunks
        // before or after the newly-inserted free chunk.
        // However, there's no *need* to do so until we actually run out of address space or until
        // a requested address is in a chunk that needs to be merged.
        // Thus, for performance, we save that for those future situations.
    }
}

allocated_chunk_inner!(AllocatedChunksPhysical, Physical);

impl Drop for AllocatedChunksPhysical {
    fn drop(&mut self) {
        if self.size_in_chunks() == 0 {
            return;
        }

        let (list, ty) =
            if physical::frame_is_in_list(&physical::RESERVED_REGIONS.lock(), self.start()) {
                (
                    &physical::FREE_RESERVED_FRAMES_LIST,
                    MemoryRegionType::Reserved,
                )
            } else {
                (&physical::FREE_GENERAL_FRAMES_LIST, MemoryRegionType::Free)
            };

        // Simply add the newly-deallocated chunk to the free frames list.
        let mut locked_list = list.lock();
        let res = locked_list.insert(ChunkRangeWrapper {
            chunks: self.chunks.clone(),
            inner: ChunkRangeWrapperPhysical { ty },
        });
        match res {
            Ok(_inserted_free_chunk) => (),
            Err(c) => error!(
                "BUG: couldn't insert deallocated chunk {:?} into free frame list",
                c
            ),
        }

        // Here, we could optionally use above `_inserted_free_chunk` to merge the adjacent (contiguous) chunks
        // before or after the newly-inserted free chunk.
        // However, there's no *need* to do so until we actually run out of address space or until
        // a requested address is in a chunk that needs to be merged.
        // Thus, for performance, we save that for those future situations.
    }
}
