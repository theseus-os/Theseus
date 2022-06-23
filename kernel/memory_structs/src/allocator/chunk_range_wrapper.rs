use core::{borrow::Borrow, cmp::Ordering, ops::Deref};

use super::AllocatedChunks;
use crate::{Chunk, ChunkRange, MemoryType};

#[derive(Debug, Clone, Eq)]
pub(crate) struct ChunkRangeWrapper<T>
where
    T: MemoryType,
{
    pub(crate) chunks: ChunkRange<T>,
    pub(crate) inner: T::ChunkRangeWrapperInner,
}

impl<T> ChunkRangeWrapper<T>
where
    T: MemoryType,
{
    const fn empty() -> Self
    where
        T::ChunkRangeWrapperInner: ~const ChunkRangeWrapperInner,
    {
        Self {
            chunks: ChunkRange::empty(),
            inner: T::ChunkRangeWrapperInner::empty(),
        }
    }

    fn as_allocated_chunks(&self) -> AllocatedChunks<T> {
        AllocatedChunks::new(self.chunks.clone())
    }
}

impl<T> const Deref for ChunkRangeWrapper<T>
where
    T: MemoryType,
{
    type Target = ChunkRange<T>;

    fn deref(&self) -> &Self::Target {
        &self.chunks
    }
}

impl<T> Ord for ChunkRangeWrapper<T>
where
    T: MemoryType,
{
    fn cmp(&self, other: &Self) -> Ordering {
        self.chunks.start().cmp(other.chunks.start())
    }
}

impl<T> PartialOrd for ChunkRangeWrapper<T>
where
    T: MemoryType,
{
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<T> PartialEq for ChunkRangeWrapper<T>
where
    T: MemoryType,
{
    fn eq(&self, other: &Self) -> bool {
        self.chunks.start() == other.chunks.start()
    }
}

impl<T> Borrow<Chunk<T>> for &'_ ChunkRangeWrapper<T>
where
    T: MemoryType,
{
    fn borrow(&self) -> &Chunk<T> {
        self.chunks.start()
    }
}

pub trait ChunkRangeWrapperInner: core::fmt::Debug + Clone + Eq {
    fn empty() -> Self;
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ChunkRangeWrapperVirtual;

impl const ChunkRangeWrapperInner for ChunkRangeWrapperVirtual {
    fn empty() -> Self {
        Self
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ChunkRangeWrapperPhysical {
    pub(crate) ty: MemoryRegionType,
}

/// Types of physical memory. See each variant's documentation.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum MemoryRegionType {
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

impl const ChunkRangeWrapperInner for ChunkRangeWrapperPhysical {
    fn empty() -> Self {
        Self {
            ty: MemoryRegionType::Unknown,
        }
    }
}
