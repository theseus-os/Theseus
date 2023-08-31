//! This crate contains basic types used for memory management.
//!
//! The types of interest are divided into three categories:
//! 1. addresses: `VirtualAddress` and `PhysicalAddress`.
//! 2. "chunk" types: `Page` and `Frame`.
//! 3. ranges of chunks: `PageRange` and `FrameRange`.  

#![no_std]
#![feature(step_trait)]
#![feature(int_roundings)]
#![allow(incomplete_features)]
#![feature(adt_const_params)]

#[cfg(test)]
mod test;

use core::{
    cmp::{min, max},
    fmt,
    iter::Step,
    marker::{ConstParamTy, PhantomData},
    ops::{Add, AddAssign, Deref, DerefMut, Sub, SubAssign},
};
use kernel_config::memory::{MAX_PAGE_NUMBER, PAGE_SIZE};
use zerocopy::FromBytes;
use paste::paste;
use derive_more::*;
use range_inclusive::{RangeInclusive, RangeInclusiveIterator};

/// Enum used to indicate the size of a page or frame.
#[derive(Debug)]
pub enum MemChunkSize {
    Normal4K,
    Huge2M,
    Huge1G,
}

/// Trait used to group the size marker structs that paging-related types are parameterized with.
pub trait PageSize: Ord + PartialOrd + Clone {
    const SIZE: MemChunkSize;
    const NUM_4K_PAGES: usize;
    const SIZE_IN_BYTES: usize;
}

/// Marker struct used to indicate the default page size of 4KiB.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Page4KiB;
impl PageSize for Page4KiB {
    const SIZE: MemChunkSize = MemChunkSize::Normal4K;
    const NUM_4K_PAGES: usize = 1;
    const SIZE_IN_BYTES: usize = PAGE_SIZE;
}

/// Marker struct used to indicate a page size of 2MiB.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Page2MiB;
impl PageSize for Page2MiB {
    const SIZE: MemChunkSize = MemChunkSize::Huge2M;
    const NUM_4K_PAGES: usize = 512;
    const SIZE_IN_BYTES: usize = PAGE_SIZE * 512;
}

/// Marker struct used to indicate a page size of 1GiB.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Page1GiB;
impl PageSize for Page1GiB {
    const SIZE: MemChunkSize = MemChunkSize::Huge1G;
    const NUM_4K_PAGES: usize = 512 * 512;
    const SIZE_IN_BYTES: usize = PAGE_SIZE * 512 * 512;
}

/// The possible states that a range of exclusively-owned pages or frames can be in.
#[derive(PartialEq, Eq, ConstParamTy)]
pub enum MemoryState {
    /// Memory is free and owned by the allocator
    Free,
    /// Memory is allocated and can be used for a mapping
    Allocated,
    /// Memory is mapped (PTE has been set)
    Mapped,
    /// Memory has been unmapped (PTE has been cleared)
    Unmapped
}

/// A macro for defining `VirtualAddress` and `PhysicalAddress` structs
/// and implementing their common traits, which are generally identical.
macro_rules! implement_address {
    ($TypeName:ident, $desc:literal, $prefix:literal, $is_canonical:ident, $canonicalize:ident, $chunk:ident) => {
        paste! { // using the paste crate's macro for easy concatenation

            #[doc = "A " $desc " memory address, which is a `usize` under the hood."]
            #[derive(
                Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, 
                Binary, Octal, LowerHex, UpperHex, 
                BitAnd, BitOr, BitXor, BitAndAssign, BitOrAssign, BitXorAssign, 
                Add, Sub, AddAssign, SubAssign,
                FromBytes,
            )]
            #[repr(transparent)]
            pub struct $TypeName(usize);

            impl $TypeName {
                #[doc = "Creates a new `" $TypeName "`, returning an error if the address is not canonical.\n\n \
                    This is useful for checking whether an address is valid before using it. 
                    For example, on x86_64, virtual addresses are canonical
                    if their upper bits `(64:48]` are sign-extended from bit 47,
                    and physical addresses are canonical if their upper bits `(64:52]` are 0."]
                pub fn new(addr: usize) -> Option<$TypeName> {
                    if $is_canonical(addr) { Some($TypeName(addr)) } else { None }
                }

                #[doc = "Creates a new `" $TypeName "` that is guaranteed to be canonical."]
                pub const fn new_canonical(addr: usize) -> $TypeName {
                    $TypeName($canonicalize(addr))
                }

                #[doc = "Creates a new `" $TypeName "` with a value 0."]
                pub const fn zero() -> $TypeName {
                    $TypeName(0)
                }

                #[doc = "Returns the underlying `usize` value for this `" $TypeName "`."]
                #[inline]
                pub const fn value(&self) -> usize {
                    self.0
                }

                #[doc = "Returns the offset from the " $chunk " boundary specified by this `"
                    $TypeName ".\n\n \
                    For example, if the [`PAGE_SIZE`] is 4096 (4KiB), then this will return
                    the least significant 12 bits `(12:0]` of this `" $TypeName "`."]
                pub const fn [<$chunk _offset>](&self) -> usize {
                    self.0 & (PAGE_SIZE - 1)
                }
            }
            impl fmt::Debug for $TypeName {
                fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                    write!(f, concat!($prefix, "{:#X}"), self.0)
                }
            }
            impl fmt::Display for $TypeName {
                fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                    write!(f, "{:?}", self)
                }
            }
            impl fmt::Pointer for $TypeName {
                fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                    write!(f, "{:?}", self)
                }
            }
            impl Add<usize> for $TypeName {
                type Output = $TypeName;
                fn add(self, rhs: usize) -> $TypeName {
                    $TypeName::new_canonical(self.0.saturating_add(rhs))
                }
            }
            impl AddAssign<usize> for $TypeName {
                fn add_assign(&mut self, rhs: usize) {
                    *self = $TypeName::new_canonical(self.0.saturating_add(rhs));
                }
            }
            impl Sub<usize> for $TypeName {
                type Output = $TypeName;
                fn sub(self, rhs: usize) -> $TypeName {
                    $TypeName::new_canonical(self.0.saturating_sub(rhs))
                }
            }
            impl SubAssign<usize> for $TypeName {
                fn sub_assign(&mut self, rhs: usize) {
                    *self = $TypeName::new_canonical(self.0.saturating_sub(rhs));
                }
            }

            #[allow(clippy::from_over_into)]
            impl Into<usize> for $TypeName {
                #[inline]
                fn into(self) -> usize {
                    self.0
                }
            }
        }
    };
}

#[cfg(target_arch = "x86_64")]
mod canonical_address {
    const CANONICAL_VIRT_ADDR_MASK: usize = 0x0000_7FFF_FFFF_FFFF;
    const CANONICAL_PHYS_ADDR_MASK: usize = 0x000F_FFFF_FFFF_FFFF;

    /// Returns whether the given virtual address value is canonical.
    ///
    /// On x86_64, virtual addresses must have their 16 most-significant bits
    /// be sign-extended from bit 47.
    #[inline]
    pub const fn is_canonical_virtual_address(virt_addr: usize) -> bool {
        let upper17 = virt_addr & !CANONICAL_VIRT_ADDR_MASK;
        upper17 == 0 || upper17 == !CANONICAL_VIRT_ADDR_MASK
    }

    /// Returns a canonicalized instance of the given virtual address value.
    ///
    /// On x86_64, virtual addresses must have their 16 most-significant bits
    /// be sign-extended from bit 47.
    #[inline]
    pub const fn canonicalize_virtual_address(virt_addr: usize) -> usize {
        // match virt_addr.get_bit(47) {
        //     false => virt_addr.set_bits(48..64, 0),
        //     true =>  virt_addr.set_bits(48..64, 0xffff),
        // };

        // The below code is semantically equivalent to the above, but it works in const functions.
        ((virt_addr << 16) as isize >> 16) as usize
    }

    /// Returns whether the given phyiscal address value is canonical.
    ///
    /// On x86_64, physical addresses are 52 bits long,
    /// so their 12 most-significant bits must be cleared.
    #[inline]
    pub const fn is_canonical_physical_address(phys_addr: usize) -> bool {
        phys_addr & !CANONICAL_PHYS_ADDR_MASK == 0
    }

    /// Returns a canonicalized instance of the given phyiscal address value.
    ///
    /// On x86_64, physical addresses are 52 bits long,
    /// so their 12 most-significant bits must be cleared.
    #[inline]
    pub const fn canonicalize_physical_address(phys_addr: usize) -> usize {
        phys_addr & CANONICAL_PHYS_ADDR_MASK
    }
}

#[cfg(target_arch = "aarch64")]
mod canonical_address {
    const CANONICAL_VIRT_ADDR_MASK: usize = 0x0000_FFFF_FFFF_FFFF;
    const CANONICAL_PHYS_ADDR_MASK: usize = 0x0000_FFFF_FFFF_FFFF;

    /// Returns whether the given virtual address value is canonical.
    ///
    /// On aarch64, virtual addresses contain an address space ID (ASID),
    /// which is 8 or 16 bits long, depending on MMU config.
    ///
    /// In Theseus, we use 8-bit ASIDs, with the next 8 bits are unused.
    /// Theseus's ASID is zero, so a canonical virtual address has its
    /// 16 most-significant bits cleared (set to zero).
    #[inline]
    pub const fn is_canonical_virtual_address(virt_addr: usize) -> bool {
        virt_addr & !CANONICAL_VIRT_ADDR_MASK == 0
    }

    /// Returns a canonicalized instance of the given virtual address value.
    ///
    /// On aarch64, virtual addresses contain an address space ID (ASID),
    /// which is 8 or 16 bits long, depending on MMU config.
    ///
    /// In Theseus, we use 8-bit ASIDs, with the next 8 bits are unused.
    /// Theseus's ASID is zero, so a virtual address is canonicalized
    /// by clearing (setting to zero) its 16 most-significant bits.
    #[inline]
    pub const fn canonicalize_virtual_address(virt_addr: usize) -> usize {
        virt_addr & CANONICAL_VIRT_ADDR_MASK
    }

    /// Returns whether the given physical address value is canonical.
    ///
    /// On aarch64, Theseus configures the MMU to use 48-bit physical addresses.
    /// Thus, a canonical physical address has its 16 most-significant bits cleared.
    #[inline]
    pub const fn is_canonical_physical_address(phys_addr: usize) -> bool {
        phys_addr & !CANONICAL_PHYS_ADDR_MASK == 0
    }

    /// Returns a canonicalized instance of the given physical address value.
    ///
    /// On aarch64, Theseus configures the MMU to use 48-bit physical addresses.
    /// Thus, a physical address is canonicalized by clearing its 16 most-significant bits.
    #[inline]
    pub const fn canonicalize_physical_address(phys_addr: usize) -> usize {
        phys_addr & CANONICAL_PHYS_ADDR_MASK
    }
}

use canonical_address::*;

implement_address!(
    VirtualAddress,
    "virtual",
    "v",
    is_canonical_virtual_address,
    canonicalize_virtual_address,
    page
);

implement_address!(
    PhysicalAddress,
    "physical",
    "p",
    is_canonical_physical_address,
    canonicalize_physical_address,
    frame
);



/// A macro for defining `Page` and `Frame` structs
/// and implementing their common traits, which are generally identical.
macro_rules! implement_page_frame {
    ($TypeName:ident, $desc:literal, $prefix:literal, $address:ident) => {
        paste! { // using the paste crate's macro for easy concatenation
            #[doc = "A `" $TypeName "` is a chunk of **" $desc "** memory aligned to a [`PAGE_SIZE`] boundary."]
            #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
            pub struct $TypeName<P: PageSize = Page4KiB> {
                number: usize,
                size: PhantomData::<P>,
            }
            impl $TypeName {
                #[doc = "Returns the `" $TypeName "` containing the given `" $address "`."]
                pub const fn containing_address(addr: $address) -> $TypeName {
                    $TypeName {
                        number: addr.value() / PAGE_SIZE,
                        size: PhantomData,
                    }
                }

                #[doc = "Returns a new `" $TypeName "` that is aligned up from this \
                    `" $TypeName "` to the nearest multiple of `alignment_4k_pages`."]
                #[doc(alias = "next_multiple_of")]
                pub const fn align_up(&self, alignment_4k_pages: usize) -> $TypeName {
                    $TypeName {
                        number: self.number.next_multiple_of(alignment_4k_pages),
                        size: PhantomData
                    }
                }
            }
            impl $TypeName<Page2MiB> {
                #[doc = "Returns the `" $TypeName "` containing the given `" $address "`."]
                pub const fn containing_address_2mb(addr: $address) -> $TypeName<Page2MiB> {
                    $TypeName {
                        number: addr.value() / (PAGE_SIZE * Page2MiB::NUM_4K_PAGES) * Page2MiB::NUM_4K_PAGES,
                        size: PhantomData::<Page2MiB>,
                    }
                }
            }
            impl $TypeName<Page1GiB> {
                #[doc = "Returns a 1GiB huge `" $TypeName "` containing the given `" $address "`."]
                pub const fn containing_address_1gb(addr: $address) -> $TypeName<Page1GiB> {
                    $TypeName {
                        number: addr.value() / (PAGE_SIZE * Page1GiB::NUM_4K_PAGES) * Page1GiB::NUM_4K_PAGES,
                        size: PhantomData::<Page1GiB>,
                    }
                }
            }
            impl<P: PageSize + 'static> $TypeName<P> {
                #[doc = "Returns the number of this `" $TypeName "`."]
                #[inline(always)]
                pub const fn number(&self) -> usize {
                    self.number
                }

                #[doc = "Returns the `" $address "` at the start of this `" $TypeName "`."]
                pub const fn start_address(&self) -> $address {
                    $address::new_canonical(self.number * PAGE_SIZE)
                }

                #[doc = "Returns a `" $TypeName "` with the same number as this `" $TypeName "` but with the size marker set to indicate a size of 4kb."]
                pub const fn as_4kb(&self) -> $TypeName {
                    $TypeName {
                        number: self.number,
                        size: PhantomData::<Page4KiB>
                    }
                }

                #[doc = "Returns the size of this `" $TypeName "`."]
                pub const fn page_size(&self) -> MemChunkSize {
                    P::SIZE
                }
            }
            impl<P: PageSize + 'static> fmt::Debug for $TypeName<P> {
                fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                    write!(f, concat!(stringify!($TypeName), "(", $prefix, "{:#X})"), self.start_address())
                }
            }
            impl<P: PageSize> Add<usize> for $TypeName<P> {
                type Output = $TypeName<P>;
                fn add(self, rhs: usize) -> $TypeName<P> {
                    // cannot exceed max page number (which is also max frame number)
                    $TypeName {
                        number: core::cmp::min(MAX_PAGE_NUMBER, self.number.saturating_add(rhs * P::NUM_4K_PAGES)),
                        size: self.size,
                    }
                }
            }
            impl<P: PageSize + Copy> AddAssign<usize> for $TypeName<P> {
                fn add_assign(&mut self, rhs: usize) {
                    *self = (*self).add(rhs * P::NUM_4K_PAGES)
                }
            }
            impl<P: PageSize> Sub<usize> for $TypeName<P> {
                type Output = $TypeName<P>;
                fn sub(self, rhs: usize) -> $TypeName<P> {
                    $TypeName {
                        number: self.number.saturating_sub(rhs * P::NUM_4K_PAGES),
                        size: self.size,
                    }
                }
            }
            impl<P: PageSize + Copy> SubAssign<usize> for $TypeName<P> {
                fn sub_assign(&mut self, rhs: usize) {
                    *self = (*self).sub(rhs);
                }
            }
            impl<P: PageSize> Step for $TypeName<P> {
                #[inline]
                fn steps_between(start: &$TypeName<P>, end: &$TypeName<P>) -> Option<usize> {
                    Step::steps_between(&start.number, &end.number).map(|n| n / P::NUM_4K_PAGES)
                }
                #[inline]
                fn forward_checked(start: $TypeName<P>, count: usize) -> Option<$TypeName<P>> {
                    Step::forward_checked(start.number, count * P::NUM_4K_PAGES).map(|n| $TypeName { number: n, size: PhantomData })
                }
                #[inline]
                fn backward_checked(start: $TypeName<P>, count: usize) -> Option<$TypeName<P>> {
                    Step::backward_checked(start.number, count * P::NUM_4K_PAGES).map(|n| $TypeName { number: n, size: PhantomData })
                }
            }
            impl TryFrom<$TypeName> for $TypeName<Page2MiB> {
                type Error = &'static str;
                fn try_from(p: $TypeName) -> Result<Self, &'static str>  {
                    if p.number %  Page2MiB::NUM_4K_PAGES == 0 {
                        return Ok ($TypeName {
                            number: p.number,
                            size: PhantomData::<Page2MiB>,
                        });
                    } else {
                        return Err("Could not convert 4KiB to 2MiB page.");
                    }
                }
            }
            impl TryFrom<$TypeName> for $TypeName<Page1GiB> {
                type Error = &'static str;
                fn try_from(p: $TypeName) -> Result<Self, &'static str> {
                     if p.number % Page1GiB::NUM_4K_PAGES == 0 {
                        return Ok ($TypeName {
                            number: p.number,
                            size: PhantomData::<Page1GiB>,
                        });
                    } else {
                        return Err("Could not convert 4KiB to 1GiB page.");
                    }
                }
            }
            impl TryFrom<$TypeName<Page2MiB>> for $TypeName<Page1GiB> {
                type Error = &'static str;
                fn try_from(p: $TypeName<Page2MiB>) -> Result<Self, &'static str> {
                     if p.number % Page1GiB::NUM_4K_PAGES == 0 { 
                        return Ok ($TypeName {
                            number: p.number,
                            size: PhantomData::<Page1GiB>,
                        });
                    } else {
                        return Err("Could not convert 2MiB to 1GiB page.");
                    }
                }
            }
            impl From<$TypeName<Page1GiB>> for $TypeName<Page2MiB> {
                fn from(p: $TypeName<Page1GiB>) -> Self { 
                    Self {                             
                        number: p.number,
                        size: PhantomData::<Page2MiB>
                    }
                }
            }
            impl From<$TypeName<Page1GiB>> for $TypeName {
                fn from(p: $TypeName<Page1GiB>) -> Self { 
                    Self {                             
                        number: p.number,
                        size: PhantomData::<Page4KiB>
                    }
                }
            }
            impl From<$TypeName<Page2MiB>> for $TypeName {
                fn from(p: $TypeName<Page2MiB>) -> Self { 
                    Self {                             
                        number: p.number,
                        size: PhantomData::<Page4KiB>
                    }
                }
            }
        }
    };
}

implement_page_frame!(Page, "virtual", "v", VirtualAddress);
implement_page_frame!(Frame, "physical", "p", PhysicalAddress);

// Implement other functions for the `Page` type that aren't relevant for `Frame.
impl<P: PageSize> Page<P> {
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
    /// Using this returned `usize` value as an index into the P1 entries list will give you the final PTE,
    /// from which you can extract the mapped [`Frame`]  using `PageTableEntry::pointed_frame()`.
    pub const fn p1_index(&self) -> usize {
        self.number & 0x1FF
    }
}



/// A macro for defining `PageRange` and `FrameRange` structs
/// and implementing their common traits, which are generally identical.
macro_rules! implement_page_frame_range {
    ($TypeName:ident, $desc:literal, $short:ident, $chunk:ident, $address:ident) => {
        paste! { // using the paste crate's macro for easy concatenation
                        
            #[doc = "A range of [`" $chunk "`]s that are contiguous in " $desc " memory."]
            #[derive(Clone, PartialEq, Eq)]
            pub struct $TypeName<P: PageSize = Page4KiB>(RangeInclusive<$chunk::<P>>);

            //impl<P: 'static + Copy> $TypeName<P> where P: PageSize {
            impl $TypeName {
                #[doc = "Creates a `" $TypeName "` that will always yield `None` when iterated."]
                pub const fn empty() -> $TypeName {
                    $TypeName::new($chunk { number: 11, size: PhantomData }, $chunk { number: 0, size: PhantomData })
                }

                #[doc = "A convenience method for creating a new `" $TypeName "` that spans \
                    all [`" $chunk "`]s from the given [`" $address "`] to an end bound based on the given size."]
                pub const fn [<from_ $short _addr>](starting_addr: $address, size_in_bytes: usize) -> $TypeName {
                    if size_in_bytes == 0 {
                        $TypeName::empty()
                    } else {
                        let start = $chunk::containing_address(starting_addr);
                        // The end bound is inclusive, hence the -1. Parentheses are needed to avoid overflow.
                        let end = $chunk::containing_address(
                            $address::new_canonical(starting_addr.value() + (size_in_bytes - 1))
                        );
                        $TypeName::new(start, end)
                    }
                }
            }
            impl<P: 'static + Copy> $TypeName<P> where P: PageSize {
                #[doc = "Returns the number of [`" $chunk "`]s covered by this iterator.\n\n \
                    Use this instead of [`Iterator::count()`] method. \
                    This is instant, because it doesn't need to iterate over each entry, unlike normal iterators."]
                pub const fn [<size_in_ $chunk:lower s>](&self) -> usize {
                    // add 1 because it's an inclusive range
                    (self.0.end().number + 1).saturating_sub(self.0.start().number) / P::NUM_4K_PAGES
                }

                #[doc = "Returns a new separate `" $TypeName "` that is extended to include the given [`" $chunk "`]."]
                pub fn to_extended(&self, to_include: $chunk<P>) -> $TypeName<P> {
                    // if the current range was empty, return a new range containing only the given page/frame
                    if self.is_empty() {
                        return $TypeName::new(to_include.clone(), to_include);
                    }
                    let start = core::cmp::min(self.0.start(), &to_include);
                    let end = core::cmp::max(self.0.end(), &to_include);
                    $TypeName::new(start.clone(), end.clone())
                }

                #[doc = "Returns an inclusive `" $TypeName "` representing the [`" $chunk "`]s that overlap \
                    across this `" $TypeName "` and the given other `" $TypeName "`.\n\n \
                    If there is no overlap between the two ranges, `None` is returned."]
                pub fn overlap(&self, other: &$TypeName<P>) -> Option<$TypeName<P>> {
                    let starts = max(*self.start(), *other.start());
                    let ends   = min(*self.end(),   *other.end());
                    if starts <= ends {
                        Some($TypeName::new(starts, ends))
                    } else {
                        None
                    }
                }

                #[doc = "Returns `true` if the `other` `" $TypeName "` is fully contained within this `" $TypeName "`."]
                pub fn contains_range(&self, other: &$TypeName<P>) -> bool {
                    !other.is_empty()
                    && (other.start() >= self.start())
                    && (other.end() <= self.end())
                }
            }
            impl<P: PageSize + 'static> $TypeName<P> {
                #[doc = "Creates a new range of [`" $chunk "`]s that spans from `start` to `end`, both inclusive bounds."]
                pub const fn new(start: $chunk<P>, end: $chunk<P>) -> $TypeName<P> {
                    $TypeName(RangeInclusive::new(start, end))
                }

                #[doc = "Returns `true` if this `" $TypeName "` contains the given [`" $address "`]."]
                pub const fn contains_address(&self, addr: $address) -> bool {
                    let c = <$chunk>::containing_address(addr);
                    self.0.start().number <= c.number
                        && c.number <= self.0.end().number
                }

                #[doc = "Returns the [`" $address "`] of the starting [`" $chunk "`] in this `" $TypeName "`."]
                pub const fn start_address(&self) -> $address {
                    self.0.start().start_address()
                }

                #[doc = "Returns the offset of the given [`" $address "`] within this `" $TypeName "`, \
                    i.e., `addr - self.start_address()`.\n\n \
                    If the given `addr` is not covered by this range of [`" $chunk "`]s, this returns `None`.\n\n \
                    # Examples\n \
                    If the range covers addresses `0x2000` to `0x4000`, then `offset_of_address(0x3500)` would return `Some(0x1500)`."]
                pub const fn offset_of_address(&self, addr: $address) -> Option<usize> {
                    if self.contains_address(addr) {
                        Some(addr.value() - self.start_address().value())
                    } else {
                        None
                    }
                }

                #[doc = "Returns the [`" $address "`] at the given `offset` into this `" $TypeName "`within this `" $TypeName "`, \
                    i.e., `self.start_address() + offset`.\n\n \
                    If the given `offset` is not within this range of [`" $chunk "`]s, this returns `None`.\n\n \
                    # Examples\n \
                    If the range covers addresses `0x2000` through `0x3FFF`, then `address_at_offset(0x1500)` would return `Some(0x3500)`, \
                    and `address_at_offset(0x2000)` would return `None`."]
                pub const fn address_at_offset(&self, offset: usize) -> Option<$address> {
                    if offset < self.size_in_bytes() {
                        Some($address::new_canonical(self.start_address().value() + offset))
                    }
                    else {
                        None
                    }
                }

                #[doc = "Returns the size of this range in bytes."]
                pub const fn size_in_bytes(&self) -> usize {
                    self.[<size_in_ $chunk:lower s_gen>]() * PAGE_SIZE
                }

                #[doc = "Returns the number of 4KiB chunks in this range. \
                    Needed for functions in the impl block generic over chunk size P, as the compiler cannot recognize which methods to call."]
                const fn [<size_in_ $chunk:lower s_gen>](&self) -> usize {
                   // add 1 because it's an inclusive range
                   (self.0.end().number + 1).saturating_sub(self.0.start().number)
                }
                

                #[doc = "Changes this `" $TypeName "` to have a size of 4KiB. This does not perform any alignment. \
                It simply changes the marker type for usage with functions that want a range of default-sized pages."]
                pub fn as_4kb_range(&self) -> $TypeName {
                    $TypeName(RangeInclusive::new(self.start().as_4kb(), self.end().as_4kb()))
                }
            }
            impl<P: 'static + PageSize> fmt::Debug for $TypeName<P> {
                fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                    write!(f, "{:?}", self.0)
                }
            }
            impl<P: PageSize> Deref for $TypeName<P> {
                type Target = RangeInclusive<$chunk<P>>;
                fn deref(&self) -> &RangeInclusive<$chunk<P>> {
                    &self.0
                }
            }
            impl DerefMut for $TypeName {
                fn deref_mut(&mut self) -> &mut RangeInclusive<$chunk> {
                    &mut self.0
                }
            }
            impl<P: PageSize> IntoIterator for $TypeName<P> {
                type Item = $chunk<P>;
                type IntoIter = RangeInclusiveIterator<$chunk<P>>;
                fn into_iter(self) -> Self::IntoIter {
                    self.0.iter()
                }
            }

            
            // #[doc = "A `" $TypeName "` that implements `Copy`"]
            // #[derive(Clone, Copy)]
            // pub struct [<Copyable $TypeName>] {
            //     start: $chunk,
            //     end: $chunk,
            // }
            #[doc = "A `" $TypeName "` that implements `Copy`"]
            #[derive(Clone, Copy)]
            pub struct [<Copyable $TypeName>]<P: PageSize = Page4KiB> {
                start: $chunk<P>,
                end: $chunk<P>,
            }
            // impl From<$TypeName> for [<Copyable $TypeName>] {
            //     fn from(r: $TypeName) -> Self {
            //         Self { start: *r.start(), end: *r.end() }
            //     }
            // }
            // impl From<[<Copyable $TypeName>]> for $TypeName {
            //     fn from(cr: [<Copyable $TypeName>]) -> Self {
            //         Self::new(cr.start, cr.end)
            //     }
            // }
            impl<P: PageSize + Copy + 'static> From<$TypeName<P>> for [<Copyable $TypeName>]<P> {
                fn from(r: $TypeName<P>) -> Self {
                    Self { start: *r.start(), end: *r.end() }
                }
            }
            impl<P: PageSize + Copy + 'static> From<[<Copyable $TypeName>]<P>> for $TypeName<P> {
                fn from(cr: [<Copyable $TypeName>]<P>) -> Self {
                    Self::new(cr.start, cr.end)
                }
            }
            impl TryFrom<$TypeName> for $TypeName<Page2MiB> {
                type Error = &'static str;
                fn try_from(p: $TypeName) -> Result<Self, &'static str> {
                    if (p.[<size_in_ $chunk:lower s>]() - 1) % 512 == 0 {
                        return Ok(Self::new(
                            $chunk::<Page2MiB>::try_from(*p.start())?,
                            $chunk::<Page2MiB>::try_from(*p.end())?
                        ));
                    } else {
                        return Err("Could not convert 4KiB page range into 2MiB page range.");
                    }
                }
            }
            impl TryFrom<$TypeName> for $TypeName<Page1GiB> {
                type Error = &'static str;
                fn try_from(p: $TypeName) -> Result<Self, &'static str> {
                    if (p.[<size_in_ $chunk:lower s>]() - 1) % (512 * 512) == 0 {
                        return Ok(Self::new(
                            $chunk::<Page1GiB>::try_from(*p.start())?,
                            $chunk::<Page1GiB>::try_from(*p.end())?
                        ));
                    } else {
                        return Err("Could not convert 4KiB page range into 1GiB page range.");
                    }
                }
            }
        }
    };
}

implement_page_frame_range!(PageRange, "virtual", virt, Page, VirtualAddress);
implement_page_frame_range!(FrameRange, "physical", phys, Frame, PhysicalAddress);
