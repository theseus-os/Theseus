use crate::{Address, MemoryType, Virtual};
use core::{
    fmt,
    iter::Step,
    marker::PhantomData,
    ops::{Add, AddAssign, Sub, SubAssign},
};
use kernel_config::memory::{MAX_PAGE_NUMBER, PAGE_SIZE};

/// A chunk of either physical or virtual memory aligned to a [`PAGE_SIZE`] boundary.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Chunk<T>
where
    T: MemoryType,
{
    number: usize,
    phantom_data: PhantomData<fn() -> T>,
}

impl<T> Chunk<T>
where
    T: MemoryType,
{
    pub const MIN: Self = Self::containing_address(Address::<T>::MIN);

    pub const MAX: Self = Self::containing_address(Address::<T>::MAX);

    #[inline]
    pub const fn new(number: usize) -> Self {
        Self {
            number,
            phantom_data: PhantomData,
        }
    }

    /// Returns the address at the start of this chunk.
    #[inline]
    pub const fn start_address(&self) -> Address<T>
    where
        T: ~const MemoryType,
    {
        Address::<T>::new_canonical(self.number() * PAGE_SIZE)
    }

    /// Returns the number of this chunk.
    #[inline]
    pub const fn number(&self) -> usize {
        self.number
    }

    /// Returns the chunk containing the given address.
    #[inline]
    pub const fn containing_address(address: Address<T>) -> Self {
        Self::new(address.value() / PAGE_SIZE)
    }
}

impl<T> fmt::Debug for Chunk<T>
where
    T: MemoryType,
{
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "({}{:#X})", T::PREFIX, self.start_address())
    }
}

impl<T> const Add<usize> for Chunk<T>
where
    T: MemoryType,
{
    type Output = Self;

    #[inline]
    fn add(self, rhs: usize) -> Self
    where
        T: ~const MemoryType,
    {
        // Cannot exceed max page number (which is also max frame number)

        // https://github.com/rust-lang/rfcs/pull/2632 (core::cmp::min isn't const)
        let temp = self.number.saturating_add(rhs);

        Self::new(if MAX_PAGE_NUMBER <= temp {
            MAX_PAGE_NUMBER
        } else {
            temp
        })
    }
}

impl<T> AddAssign<usize> for Chunk<T>
where
    T: MemoryType,
{
    #[inline]
    fn add_assign(&mut self, rhs: usize) {
        // Cannot exceed max page number (which is also max frame number)

        // https://github.com/rust-lang/rfcs/pull/2632 (core::cmp::min isn't const)
        let temp = self.number.saturating_add(rhs);

        *self = Self::new(if MAX_PAGE_NUMBER <= temp {
            MAX_PAGE_NUMBER
        } else {
            temp
        });
    }
}

impl<T> const Sub<usize> for Chunk<T>
where
    T: MemoryType,
{
    type Output = Self;

    #[inline]
    fn sub(self, rhs: usize) -> Self
    where
        T: ~const MemoryType,
    {
        Self::new(self.number.saturating_sub(rhs))
    }
}

impl<T> SubAssign<usize> for Chunk<T>
where
    T: MemoryType,
{
    #[inline]
    fn sub_assign(&mut self, rhs: usize) {
        *self = Self::new(self.number.saturating_sub(rhs));
    }
}

/// Implementing [`Step`] allows [`Chunk`] to be used as an [`Iterator`].
impl<T> Step for Chunk<T>
where
    T: MemoryType,
{
    #[inline]
    fn steps_between(start: &Self, end: &Self) -> Option<usize> {
        Step::steps_between(&start.number, &end.number)
    }

    #[inline]
    fn forward_checked(start: Self, count: usize) -> Option<Self> {
        Step::forward_checked(start.number, count).map(|n| Self::new(n))
    }

    #[inline]
    fn backward_checked(start: Self, count: usize) -> Option<Self> {
        Step::backward_checked(start.number, count).map(|n| Self::new(n))
    }
}

// Implement other functions for the virtual chunks (i.e. pages) that aren't relevant for physical
// chunks (i.e. frames).
impl Chunk<Virtual> {
    /// Returns the 9-bit part of this `Page`'s [`VirtualAddress`] that is the index into the P4 page table entries list.
    pub const fn p4_index(&self) -> usize {
        (self.number >> 27) & 0x1FF
    }

    /// Returns the 9-bit part of this `Page`'s [`VirtualAddress`] that is the index into the P3 page table entries list.
    pub const fn p3_index(&self) -> usize {
        (self.number >> 18) & 0x1FF
    }

    /// Returns the 9-bit part of this `Page`'s [`VirtualAddress`] that is the index into the P2 page table entries list.
    pub const fn p2_index(&self) -> usize {
        (self.number >> 9) & 0x1FF
    }

    /// Returns the 9-bit part of this `Page`'s [`VirtualAddress`] that is the index into the P1 page table entries list.
    ///
    /// Returns a new seperate range that is extended to include the given chunk.
    /// Using this returned `usize` value as an index into the P1 entries list will give you the final PTE,
    /// from which you can extract the mapped [`Frame`]  using `PageTableEntry::pointed_frame()`.
    pub const fn p1_index(&self) -> usize {
        self.number & 0x1FF
    }
}
