use crate::{Address, Chunk, MemoryType};
use core::{
    cmp::{max, min},
    marker::PhantomData,
    ops::{Deref, DerefMut, RangeInclusive},
};
use kernel_config::memory::PAGE_SIZE;

#[derive(Clone, Eq, PartialEq, Debug)]
pub struct ChunkRange<T>
where
    T: MemoryType,
{
    inner: RangeInclusive<Chunk<T>>,
    phantom_data: PhantomData<fn() -> T>,
}

impl<T> ChunkRange<T>
where
    T: MemoryType,
{
    /// Creates a new chunk that spans from `start` to `end`, both inclusize bounds.
    #[inline]
    pub const fn new(start: Chunk<T>, end: Chunk<T>) -> Self {
        Self {
            inner: RangeInclusive::new(start, end),
            phantom_data: PhantomData,
        }
    }

    /// Creates a chunk range that will always yield [`Option::None`] when iterated.
    #[inline]
    pub const fn empty() -> Self {
        Self::new(Chunk::new(1), Chunk::new(0))
    }

    /// A convenience method for creating a new chunk range that spans all chunks from the given
    /// address to an end bound based on the given size.
    #[inline]
    pub const fn from_address(starting_addr: Address<T>, size_in_bytes: usize) -> Self {
        assert!(size_in_bytes > 0);
        let start = Chunk::containing_address(starting_addr);
        // The end bound is inclusive, hence the -1. Parentheses are needed to avoid overflow.
        let end = Chunk::containing_address(starting_addr + (size_in_bytes - 1));
        Self::new(start, end)
    }

    /// Returns the address of the starting chunk in this range.
    #[inline]
    pub const fn start_address(&self) -> Address<T>
    where
        T: ~const MemoryType,
    {
        self.start().start_address()
    }

    /// Returns the number of chunks covered by this iterator.
    ///
    /// Use this instead of the [`Iterator::count()`] method. This is instant, because it doesn't
    /// need to iterate over each entry, unlike [`Iterator::count()`].
    #[inline]
    pub const fn size_in_chunks(&self) -> usize {
        // Add 1 because it's an inclusive range.
        (self.end().number() + 1).saturating_sub(self.start().number())
    }

    /// Returns the size of this range in number of bytes.
    #[inline]
    pub const fn size_in_bytes(&self) -> usize {
        self.size_in_chunks() * PAGE_SIZE
    }

    /// Returns true if the given address is contained by self.
    #[inline]
    pub fn contains_address(&self, address: Address<T>) -> bool {
        self.inner
            .contains(&Chunk::<T>::containing_address(address))
    }

    /// Returns the offset of the given address within this range (i.e. `addr - self.start_address()`).
    ///
    /// If the given address is not covered by this range of chunks, the function returns
    /// [`Option::None`].
    ///
    /// # Examples
    /// If the range covers addresses `0x2000` to `0x4000` then `offset_of_address(0x3500)` would
    /// return `Some(0x1500)`.
    #[inline]
    pub fn offset_of_address(&self, addr: Address<T>) -> Option<usize> {
        if self.contains_address(addr) {
            Some(addr.value() - self.start_address().value())
        } else {
            None
        }
    }

    /// Returns the address at the given `offset` into this chunk range (i.e. `addr - self.start_address()`).
    ///
    /// If the given `offset` is not within this range of chunks, the function returns [`Option::None`]
    ///
    /// # Examples
    /// If the range covers addresses `0x2000` to `0x4000` then `address_at_offset(0x1500)` would
    /// return `Some(0x3500)`.
    #[inline]
    pub const fn address_at_offset(&self, offset: usize) -> Option<Address<T>>
    where
        T: ~const MemoryType,
    {
        if offset <= self.size_in_bytes() {
            Some(self.start_address() + offset)
        } else {
            None
        }
    }

    /// Returns a new separate range that is extended to include the given chunk.
    #[inline]
    pub fn to_extended(&self, to_include: Chunk<T>) -> Self {
        // If the current range was empty, return a new range containing only the given page/frame.
        if self.is_empty() {
            return Self::new(to_include, to_include);
        }
        let start = min(self.start(), &to_include);
        let end = max(self.end(), &to_include);
        Self::new(*start, *end)
    }

    /// Returns an inclusive chunk range representing the chunks that overlap across this chunk
    /// range and the given chunk range.
    ///
    /// If there is no overlap between the two ranges, [`Option::None`] is returned.
    #[inline]
    pub fn overlap(&self, other: &Self) -> Option<Self> {
        // TODO: This function could be constified but it would require inlining `min` and `max`,
        // and manually implementing `const Ord` for `Chunk`. I don't think it's worth the hassle.
        let starts = max(*self.start(), *other.start());
        let ends = min(*self.end(), *other.end());
        if starts <= ends {
            Some(Self::new(starts, ends))
        } else {
            None
        }
    }
}

impl<T> const Deref for ChunkRange<T>
where
    T: MemoryType,
{
    type Target = RangeInclusive<Chunk<T>>;

    #[inline]
    fn deref(&self) -> &RangeInclusive<Chunk<T>> {
        &self.inner
    }
}

impl<T> DerefMut for ChunkRange<T>
where
    T: MemoryType,
{
    #[inline]
    fn deref_mut(&mut self) -> &mut RangeInclusive<Chunk<T>> {
        &mut self.inner
    }
}

impl<T> const IntoIterator for ChunkRange<T>
where
    T: MemoryType,
{
    type Item = Chunk<T>;

    type IntoIter = RangeInclusive<Chunk<T>>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.inner
    }
}
