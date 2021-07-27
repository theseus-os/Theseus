//! This crate contains common types used for memory mapping. 

#![no_std]
#![feature(step_trait, step_trait_ext)]

extern crate kernel_config;
extern crate multiboot2;
extern crate xmas_elf;
#[macro_use] extern crate derive_more;
extern crate bit_field;
#[cfg(target_arch = "x86_64")]
extern crate entryflags_x86_64;
extern crate zerocopy;
extern crate paste;


use bit_field::BitField;
use core::{
    cmp::{min, max},
    fmt,
    iter::Step,
    ops::{Add, AddAssign, Deref, DerefMut, RangeInclusive, Sub, SubAssign},
};
use kernel_config::memory::{MAX_PAGE_NUMBER, PAGE_SIZE};
#[cfg(target_arch = "x86_64")]
pub use entryflags_x86_64::EntryFlags;
use zerocopy::FromBytes;
use paste::paste;


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
            impl Into<usize> for $TypeName {
                #[inline]
                fn into(self) -> usize {
                    self.0
                }
            }
        }
    };
}

#[inline]
fn is_canonical_virtual_address(virt_addr: usize) -> bool {
    match virt_addr.get_bits(47..64) {
        0 | 0b1_1111_1111_1111_1111 => true,
        _ => false,
    }
}

#[inline]
const fn canonicalize_virtual_address(virt_addr: usize) -> usize {
    // match virt_addr.get_bit(47) {
    //     false => virt_addr.set_bits(48..64, 0),
    //     true =>  virt_addr.set_bits(48..64, 0xffff),
    // };

    // The below code is semantically equivalent to the above, but it works in const functions.
    ((virt_addr << 16) as isize >> 16) as usize
}

#[inline]
fn is_canonical_physical_address(phys_addr: usize) -> bool {
    match phys_addr.get_bits(52..64) {
        0 => true,
        _ => false,
    }
}

#[inline]
const fn canonicalize_physical_address(phys_addr: usize) -> usize {
    phys_addr & 0x000F_FFFF_FFFF_FFFF
}

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
            pub struct $TypeName {
                number: usize,
            }

            impl $TypeName {
                #[doc = "Returns the `" $address "` at the start of this `" $TypeName "`."]
                pub const fn start_address(&self) -> $address {
                    $address::new_canonical(self.number * PAGE_SIZE)
                }

                #[doc = "Returns the number of this `" $TypeName "`."]
                #[inline(always)]
                pub const fn number(&self) -> usize {
                    self.number
                }
                
                #[doc = "Returns the `" $TypeName "` containing the given `" $address "`."]
                pub const fn containing_address(addr: $address) -> $TypeName {
                    $TypeName {
                        number: addr.value() / PAGE_SIZE,
                    }
                }
            }
            impl fmt::Debug for $TypeName {
                fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                    write!(f, concat!(stringify!($TypeName), "(", $prefix, "{:#X})"), self.start_address())
                }
            }
            impl Add<usize> for $TypeName {
                type Output = $TypeName;
                fn add(self, rhs: usize) -> $TypeName {
                    // cannot exceed max page number (which is also max frame number)
                    $TypeName {
                        number: core::cmp::min(MAX_PAGE_NUMBER, self.number.saturating_add(rhs)),
                    }
                }
            }
            impl AddAssign<usize> for $TypeName {
                fn add_assign(&mut self, rhs: usize) {
                    *self = $TypeName {
                        number: core::cmp::min(MAX_PAGE_NUMBER, self.number.saturating_add(rhs)),
                    };
                }
            }
            impl Sub<usize> for $TypeName {
                type Output = $TypeName;
                fn sub(self, rhs: usize) -> $TypeName {
                    $TypeName {
                        number: self.number.saturating_sub(rhs),
                    }
                }
            }
            impl SubAssign<usize> for $TypeName {
                fn sub_assign(&mut self, rhs: usize) {
                    *self = $TypeName {
                        number: self.number.saturating_sub(rhs),
                    };
                }
            }
            #[doc = "Implementing `Step` allows `" $TypeName "` to be used in an [`Iterator`]."]
            unsafe impl Step for $TypeName {
                #[inline]
                fn steps_between(start: &$TypeName, end: &$TypeName) -> Option<usize> {
                    Step::steps_between(&start.number, &end.number)
                }
                #[inline]
                fn forward_checked(start: $TypeName, count: usize) -> Option<$TypeName> {
                    Step::forward_checked(start.number, count).map(|n| $TypeName { number: n })
                }
                #[inline]
                fn backward_checked(start: $TypeName, count: usize) -> Option<$TypeName> {
                    Step::backward_checked(start.number, count).map(|n| $TypeName { number: n })
                }
            }

        }
    };
}

implement_page_frame!(Page, "virtual", "v", VirtualAddress);
implement_page_frame!(Frame, "physical", "p", PhysicalAddress);

// Implement other functions for the `Page` type that aren't relevant for `Frame.
impl Page {
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
        (self.number >> 0) & 0x1FF
    }
}

/// A range of `Frame`s that are contiguous in physical memory.
#[derive(Clone, PartialEq, Eq)]
pub struct FrameRange(RangeInclusive<Frame>);

impl FrameRange {
    /// Creates a new range of `Frame`s that spans from `start` to `end`,
    /// both inclusive bounds.
    pub const fn new(start: Frame, end: Frame) -> FrameRange {
        FrameRange(RangeInclusive::new(start, end))
    }

    /// Creates a FrameRange that will always yield `None`.
    pub const fn empty() -> FrameRange {
        FrameRange::new(Frame { number: 1 }, Frame { number: 0 })
    }

    /// A convenience method for creating a new `FrameRange`
    /// that spans all `Frame`s from the given physical address
    /// to an end bound based on the given size.
    pub fn from_phys_addr(starting_phys_addr: PhysicalAddress, size_in_bytes: usize) -> FrameRange {
        assert!(size_in_bytes > 0);
        let start_frame = Frame::containing_address(starting_phys_addr);
		// The end frame is an inclusive bound, hence the -1. Parentheses are needed to avoid overflow.
        let end_frame = Frame::containing_address(starting_phys_addr + (size_in_bytes - 1));
        FrameRange::new(start_frame, end_frame)
    }

    /// Returns the `PhysicalAddress` of the starting `Frame` in this `FrameRange`.
    pub const fn start_address(&self) -> PhysicalAddress {
        self.0.start().start_address()
    }

    /// Returns the number of `Frame`s covered by this iterator.
    /// Use this instead of the Iterator trait's `count()` method.
    /// This is instant, because it doesn't need to iterate over each entry, unlike normal iterators.
    pub fn size_in_frames(&self) -> usize {
        // add 1 because it's an inclusive range
        (self.0.end().number + 1).saturating_sub(self.0.start().number)
    }

    /// Whether this `FrameRange` contains the given `PhysicalAddress`.
    pub fn contains_phys_addr(&self, phys_addr: PhysicalAddress) -> bool {
        self.0.contains(&Frame::containing_address(phys_addr))
    }

    /// Returns the offset of the given `PhysicalAddress` within this `FrameRange`,
    /// i.e., the difference between `phys_addr` and `self.start()`.
    pub fn offset_from_start(&self, phys_addr: PhysicalAddress) -> Option<usize> {
        if self.contains_phys_addr(phys_addr) {
            Some(phys_addr.value() - self.start_address().value())
        } else {
            None
        }
    }

    /// Returns a new, separate `FrameRange` that is extended to include the given `Frame`.
    pub fn to_extended(&self, frame_to_include: Frame) -> FrameRange {
        // if the current FrameRange was empty, return a new FrameRange containing only the given frame_to_include
        if self.is_empty() {
            return FrameRange::new(frame_to_include.clone(), frame_to_include);
        }

        let start = core::cmp::min(self.0.start(), &frame_to_include);
        let end = core::cmp::max(self.0.end(), &frame_to_include);
        FrameRange::new(start.clone(), end.clone())
    }

    /// Returns an inclusive `FrameRange` representing the frames that overlap
    /// across this `FrameRange` and the given other `FrameRange`. 
    ///
    /// If there is no overlap between the two ranges, `None` is returned.
    pub fn overlap(&self, other: &FrameRange) -> Option<FrameRange> {
        let starts = max(*self.start(), *other.start());
        let ends   = min(*self.end(),   *other.end());
        if starts <= ends {
            Some(FrameRange::new(starts, ends))
        } else {
            None
        }
    }
}
impl fmt::Debug for FrameRange {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(f, "{:?}", self.0)
	}
}
impl Deref for FrameRange {
    type Target = RangeInclusive<Frame>;
    fn deref(&self) -> &RangeInclusive<Frame> {
        &self.0
    }
}
impl DerefMut for FrameRange {
    fn deref_mut(&mut self) -> &mut RangeInclusive<Frame> {
        &mut self.0
    }
}

impl IntoIterator for FrameRange {
    type Item = Frame;
    type IntoIter = RangeInclusive<Frame>;

    fn into_iter(self) -> Self::IntoIter {
        self.0
    }
}


/// An inclusive range of `Page`s that are contiguous in virtual memory.
#[derive(Clone, PartialEq, Eq)]
pub struct PageRange(RangeInclusive<Page>);

impl PageRange {
    /// Creates a new range of `Page`s that spans from `start` to `end`,
    /// both inclusive bounds.
    pub const fn new(start: Page, end: Page) -> PageRange {
        PageRange(RangeInclusive::new(start, end))
    }

    /// Creates a PageRange that will always yield `None`.
    pub const fn empty() -> PageRange {
        PageRange::new(Page { number: 1 }, Page { number: 0 })
    }

    /// A convenience method for creating a new `PageRange`
    /// that spans all `Page`s from the given virtual address
    /// to an end bound based on the given size.
    pub fn from_virt_addr(starting_virt_addr: VirtualAddress, size_in_bytes: usize) -> PageRange {
        assert!(size_in_bytes > 0);
        let start_page = Page::containing_address(starting_virt_addr);
		// The end page is an inclusive bound, hence the -1. Parentheses are needed to avoid overflow.
        let end_page = Page::containing_address(starting_virt_addr + (size_in_bytes - 1));
        PageRange::new(start_page, end_page)
    }

    /// Returns the `VirtualAddress` of the starting `Page`.
    pub const fn start_address(&self) -> VirtualAddress {
        self.0.start().start_address()
    }

    /// Returns the size in number of `Page`s.
    /// Use this instead of the Iterator trait's `count()` method.
    /// This is instant, because it doesn't need to iterate over each `Page`, unlike normal iterators.
    pub const fn size_in_pages(&self) -> usize {
        // add 1 because it's an inclusive range
        (self.0.end().number + 1).saturating_sub(self.0.start().number)
    }

    /// Returns the size in number of bytes.
    pub const fn size_in_bytes(&self) -> usize {
        self.size_in_pages() * PAGE_SIZE
    }

    /// Whether this `PageRange` contains the given `VirtualAddress`.
    pub fn contains_virt_addr(&self, virt_addr: VirtualAddress) -> bool {
        self.0.contains(&Page::containing_address(virt_addr))
    }

    /// Returns the offset of the given `VirtualAddress` within this `PageRange`,
    /// i.e., the difference between `virt_addr` and `self.start_address()`.
    /// If the given `VirtualAddress` is not covered by this range of `Page`s, this returns `None`.
    ///  
    /// # Examples
    /// If the page range covered addresses `0x2000` to `0x4000`, then calling
    /// `offset_of_address(0x3500)` would return `Some(0x1500)`.
    pub fn offset_of_address(&self, virt_addr: VirtualAddress) -> Option<usize> {
        if self.contains_virt_addr(virt_addr) {
            Some(virt_addr.value() - self.start_address().value())
        } else {
            None
        }
    }

    /// Returns the `VirtualAddress` at the given `offset` into this mapping,  
    /// If the given `offset` is not covered by this range of `Page`s, this returns `None`.
    ///  
    /// # Examples
    /// If the page range covered addresses `0xFFFFFFFF80002000` to `0xFFFFFFFF80004000`,
    /// then calling `address_at_offset(0x1500)` would return `Some(0xFFFFFFFF80003500)`.
    pub fn address_at_offset(&self, offset: usize) -> Option<VirtualAddress> {
        if offset <= self.size_in_bytes() {
            Some(self.start_address() + offset)
        }
        else {
            None
        }
    }
}
impl fmt::Debug for PageRange {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(f, "{:?}", self.0)
	}
}
impl Deref for PageRange {
    type Target = RangeInclusive<Page>;
    fn deref(&self) -> &RangeInclusive<Page> {
        &self.0
    }
}
impl DerefMut for PageRange {
    fn deref_mut(&mut self) -> &mut RangeInclusive<Page> {
        &mut self.0
    }
}

impl IntoIterator for PageRange {
    type Item = Page;
    type IntoIter = RangeInclusive<Page>;

    fn into_iter(self) -> Self::IntoIter {
        self.0
    }
}


/// The address bounds and mapping flags of a section's memory region.
#[derive(Debug)]
pub struct SectionMemoryBounds {
    /// The starting virtual address and physical address.
    pub start: (VirtualAddress, PhysicalAddress),
    /// The ending virtual address and physical address.
    pub end: (VirtualAddress, PhysicalAddress),
    /// The page table entry flags that should be used for mapping this section.
    pub flags: EntryFlags,
}

/// The address bounds and flags of the initial kernel sections that need mapping. 
/// 
/// It contains three main items, in which each item includes all sections that have identical flags:
/// * The `.text` section bounds cover all sections that are executable.
/// * The `.rodata` section bounds cover those that are read-only (.rodata, .gcc_except_table, .eh_frame).
/// * The `.data` section bounds cover those that are writable (.data, .bss).
/// 
/// It also contains the bounds of the initial page table (root p4 frame) and 
/// the initial stack, which are maintained separately.
#[derive(Debug)]
pub struct AggregatedSectionMemoryBounds {
   pub text:        SectionMemoryBounds,
   pub rodata:      SectionMemoryBounds,
   pub data:        SectionMemoryBounds,
   pub page_table:  SectionMemoryBounds,
   pub stack:       SectionMemoryBounds,
}
