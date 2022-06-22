use crate::MemoryType;
use core::{
    fmt::{self, Binary, Debug, Display, LowerHex, Octal, Pointer, UpperHex},
    hash::Hash,
    marker::PhantomData,
    ops::{
        Add, AddAssign, BitAnd, BitAndAssign, BitOr, BitOrAssign, BitXor, BitXorAssign, Sub,
        SubAssign,
    },
};
use kernel_config::memory::PAGE_SIZE;
use zerocopy::FromBytes;

/// Either a physical or virtual address which is a [`usize`] under the hood.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, FromBytes)]
#[repr(transparent)]
pub struct Address<T>
where
    T: MemoryType,
{
    address: usize,
    phantom_data: PhantomData<fn() -> T>,
}

impl<T> Address<T>
where
    T: MemoryType,
{
    /// Creates a new address, returning an error if the address is not canonical.
    ///
    /// This is useful for checking whether an address is valid before using it. For example on
    /// x86_64, virtual addresses are canonical if their upper bits `(64:68]` are sign-extended
    /// from bit 47, and physical addresses are canonical if their upper bits `(64:52]` are 0.
    #[inline]
    pub const fn new(address: usize) -> Option<Self>
    where
        T: ~const MemoryType,
    {
        if T::is_canonical_address(address) {
            Some(Self {
                address,
                phantom_data: PhantomData,
            })
        } else {
            None
        }
    }

    /// Creates a new address that is guaranteed to be canonical.
    pub const fn new_canonical(address: usize) -> Self
    where
        T: ~const MemoryType,
    {
        Self {
            address: T::canonicalize_address(address),
            phantom_data: PhantomData,
        }
    }

    /// Creates a new address with a value of zero.
    #[inline]
    pub const fn zero() -> Self {
        Self {
            address: 0,
            phantom_data: PhantomData,
        }
    }

    /// Returns the underlying usize value.
    #[inline]
    pub const fn value(&self) -> usize {
        self.address
    }

    /// Returns the offsets from the chunk boundary specified by this address.
    ///
    /// For example if the [`PAGE_SIZE`] is 4096 (4KiB) then this will return the least significant
    /// 12 bits `(12:0]` of the address.
    #[inline]
    pub const fn chunk_offset(&self) -> usize {
        self.value() & (PAGE_SIZE - 1)
    }
}

// Formatting Traits

impl<T> Debug for Address<T>
where
    T: MemoryType,
{
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}{:#X}", T::PREFIX, self.address)
    }
}

macro_rules! format_impl {
    ($trait:path, $char:tt) => {
        impl<T> $trait for Address<T>
        where
            T: MemoryType,
        {
            #[inline]
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, concat!("{:", stringify!($char), "}"), self)
            }
        }
    };
}

format_impl!(Display, ?);
format_impl!(Pointer, ?);
format_impl!(Binary, b);
format_impl!(Octal, o);
format_impl!(LowerHex, x);
format_impl!(UpperHex, X);

// Bit Traits

impl<T> const BitAnd<Address<T>> for Address<T>
where
    T: MemoryType,
{
    type Output = Self;

    fn bitand(self, rhs: Self) -> Self::Output
    where
        T: ~const MemoryType,
    {
        // TODO: new_canonical or raw?
        Self::new_canonical(self.address & rhs.address)
    }
}

impl<T> BitAndAssign<Address<T>> for Address<T>
where
    T: MemoryType,
{
    fn bitand_assign(&mut self, rhs: Self) {
        // TODO: new_canonical or raw?
        *self = Self::new_canonical(self.address & rhs.address)
    }
}

impl<T> const BitOr<Address<T>> for Address<T>
where
    T: MemoryType,
{
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output
    where
        T: ~const MemoryType,
    {
        // TODO: new_canonical or raw?
        Self::new_canonical(self.address | rhs.address)
    }
}

impl<T> BitOrAssign<Address<T>> for Address<T>
where
    T: MemoryType,
{
    fn bitor_assign(&mut self, rhs: Self) {
        // TODO: new_canonical or raw?
        *self = Self::new_canonical(self.address | rhs.address)
    }
}

impl<T> const BitXor<Address<T>> for Address<T>
where
    T: MemoryType,
{
    type Output = Self;

    fn bitxor(self, rhs: Self) -> Self::Output
    where
        T: ~const MemoryType,
    {
        // TODO: new_canonical or raw?
        Self::new_canonical(self.address ^ rhs.address)
    }
}

impl<T> BitXorAssign<Address<T>> for Address<T>
where
    T: MemoryType,
{
    fn bitxor_assign(&mut self, rhs: Self) {
        // TODO: new_canonical or raw?
        *self = Self::new_canonical(self.address ^ rhs.address)
    }
}

// Computation Traits (self)

impl<T> const Add<Address<T>> for Address<T>
where
    T: MemoryType,
{
    type Output = Self;

    #[inline]
    fn add(self, rhs: Self) -> Self::Output
    where
        T: ~const MemoryType,
    {
        // TODO: new_canonical or raw?
        Self::new_canonical(self.address + rhs.address)
    }
}

impl<T> AddAssign<Address<T>> for Address<T>
where
    T: MemoryType,
{
    #[inline]
    fn add_assign(&mut self, rhs: Self) {
        // TODO: new_canonical or raw?
        *self = Self::new_canonical(self.address.saturating_add(rhs.address));
    }
}

impl<T> const Sub<Address<T>> for Address<T>
where
    T: MemoryType,
{
    type Output = Self;

    #[inline]
    fn sub(self, rhs: Self) -> Self
    where
        T: ~const MemoryType,
    {
        // TODO: new_canonical or raw?
        Self::new_canonical(self.address.saturating_sub(rhs.address))
    }
}

impl<T> SubAssign<Address<T>> for Address<T>
where
    T: MemoryType,
{
    #[inline]
    fn sub_assign(&mut self, rhs: Self) {
        // TODO: new_canonical or raw?
        *self = Self::new_canonical(self.address.saturating_sub(rhs.address));
    }
}

// Computation Traits (usize)

impl<T> const Add<usize> for Address<T>
where
    T: MemoryType,
{
    type Output = Self;

    #[inline]
    fn add(self, rhs: usize) -> Self
    where
        T: ~const MemoryType,
    {
        Self::new_canonical(self.address.saturating_add(rhs))
    }
}

impl<T> AddAssign<usize> for Address<T>
where
    T: MemoryType,
{
    #[inline]
    fn add_assign(&mut self, rhs: usize) {
        *self = Self::new_canonical(self.address.saturating_add(rhs));
    }
}

impl<T> const Sub<usize> for Address<T>
where
    T: MemoryType,
{
    type Output = Self;

    #[inline]
    fn sub(self, rhs: usize) -> Self
    where
        T: ~const MemoryType,
    {
        Self::new_canonical(self.address.saturating_sub(rhs))
    }
}

impl<T> SubAssign<usize> for Address<T>
where
    T: MemoryType,
{
    #[inline]
    fn sub_assign(&mut self, rhs: usize) {
        *self = Self::new_canonical(self.address.saturating_sub(rhs));
    }
}

// Conversion Traits

impl<T> const From<Address<T>> for usize
where
    T: MemoryType,
{
    #[inline]
    fn from(address: Address<T>) -> Self {
        address.address
    }
}
