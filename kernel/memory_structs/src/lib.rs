//! This crate contains common types used for memory mapping. 

#![no_std]
#![feature(const_fn)]
#![feature(range_is_empty)]
#![feature(step_trait)]

extern crate atomic_linked_list;
extern crate heap_irq_safe;
extern crate kernel_config;
extern crate multiboot2;
extern crate xmas_elf;
#[macro_use] extern crate derive_more;
extern crate bit_field;
#[cfg(target_arch = "x86_64")]
extern crate entryflags_x86_64;

use bit_field::BitField;
use core::{
    fmt,
    iter::Step,
    mem,
    ops::{Add, AddAssign, Deref, DerefMut, RangeInclusive, Sub, SubAssign},
};
use kernel_config::memory::{MAX_PAGE_NUMBER, PAGE_SIZE};
#[cfg(target_arch = "x86_64")]
use entryflags_x86_64::EntryFlags;

/// A virtual memory address, which is a `usize` under the hood.
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, 
    Debug, Display, Binary, Octal, LowerHex, UpperHex, 
    BitAnd, BitOr, BitXor, BitAndAssign, BitOrAssign, BitXorAssign, 
    Add, Sub, AddAssign, SubAssign
)]
#[repr(transparent)]
pub struct VirtualAddress(usize);

impl VirtualAddress {
    /// Creates a new `VirtualAddress`,
    /// checking that the address is canonical,
    /// i.e., bits (64:48] are sign-extended from bit 47.
    pub fn new(virt_addr: usize) -> Result<VirtualAddress, &'static str> {
        match virt_addr.get_bits(47..64) {
            0 | 0b1_1111_1111_1111_1111 => Ok(VirtualAddress(virt_addr)),
            _ => Err("VirtualAddress bits 48-63 must be a sign-extension of bit 47"),
        }
    }

    /// Creates a new `VirtualAddress` that is guaranteed to be canonical
    /// by forcing the upper bits (64:48] to be sign-extended from bit 47.
    pub const fn new_canonical(virt_addr: usize) -> VirtualAddress {
        // match virt_addr.get_bit(47) {
        //     false => virt_addr.set_bits(48..64, 0),
        //     true => virt_addr.set_bits(48..64, 0xffff),
        // };
        //
        // The below code is semantically equivalent to the above, but it works in const functions.
        VirtualAddress((virt_addr.wrapping_mul(0x1_0000) as isize / 0x1_0000) as usize)
    }

    /// Creates a VirtualAddress with the value 0.
    pub const fn zero() -> VirtualAddress {
        VirtualAddress(0)
    }

    /// Returns the underlying `usize` value for this `VirtualAddress`.
    #[inline]
    pub const fn value(&self) -> usize {
        self.0
    }

    /// Returns the offset that this VirtualAddress specifies into its containing memory Page.
    ///
    /// For example, if the PAGE_SIZE is 4KiB, then this will return
    /// the least significant 12 bits (12:0] of this VirtualAddress.
    pub fn page_offset(&self) -> usize {
        self.0 & (PAGE_SIZE - 1)
    }
}

impl fmt::Pointer for VirtualAddress {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:p}", self.0 as *const usize)
    }
}

impl Add<usize> for VirtualAddress {
    type Output = VirtualAddress;

    fn add(self, rhs: usize) -> VirtualAddress {
        VirtualAddress::new_canonical(self.0.saturating_add(rhs))
    }
}

impl AddAssign<usize> for VirtualAddress {
    fn add_assign(&mut self, rhs: usize) {
        *self = VirtualAddress::new_canonical(self.0.saturating_add(rhs));
    }
}

impl Sub<usize> for VirtualAddress {
    type Output = VirtualAddress;

    fn sub(self, rhs: usize) -> VirtualAddress {
        VirtualAddress::new_canonical(self.0.saturating_sub(rhs))
    }
}

impl SubAssign<usize> for VirtualAddress {
    fn sub_assign(&mut self, rhs: usize) {
        *self = VirtualAddress::new_canonical(self.0.saturating_sub(rhs));
    }
}

impl From<VirtualAddress> for usize {
    #[inline]
    fn from(virt_addr: VirtualAddress) -> usize {
        virt_addr.0
    }
}


/// A physical memory address, which is a `usize` under the hood.
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default,
    Debug, Display, Binary, Octal, LowerHex, UpperHex, 
    BitAnd, BitOr, BitXor, BitAndAssign, BitOrAssign, BitXorAssign,
    Add, Sub, Mul, Div, Rem, Shr, Shl,
    AddAssign, SubAssign, MulAssign, DivAssign, RemAssign, ShrAssign, ShlAssign,
)]
#[repr(transparent)]
pub struct PhysicalAddress(usize);

impl PhysicalAddress {
    /// Creates a new `PhysicalAddress`,
    /// checking that the bits (64:52] are 0.
    pub fn new(phys_addr: usize) -> Result<PhysicalAddress, &'static str> {
        match phys_addr.get_bits(52..64) {
            0 => Ok(PhysicalAddress(phys_addr)),
            _ => Err("PhysicalAddress bits 52-63 must be zero"),
        }
    }

    /// Creates a new `PhysicalAddress` that is guaranteed to be canonical
    /// by forcing the upper bits (64:52] to be 0.
    pub fn new_canonical(mut phys_addr: usize) -> PhysicalAddress {
        phys_addr.set_bits(52..64, 0);
        PhysicalAddress(phys_addr)
    }

    /// Returns the underlying `usize` value for this `PhysicalAddress`.
    #[inline]
    pub fn value(&self) -> usize {
        self.0
    }

    /// Creates a PhysicalAddress with the value 0.
    pub const fn zero() -> PhysicalAddress {
        PhysicalAddress(0)
    }

    /// Returns the offset that this PhysicalAddress specifies into its containing memory Frame.
    ///
    /// For example, if the PAGE_SIZE is 4KiB, then this will return
    /// the least significant 12 bits (12:0] of this PhysicalAddress.
    pub fn frame_offset(&self) -> usize {
        self.0 & (PAGE_SIZE - 1)
    }
}


impl Add<usize> for PhysicalAddress {
    type Output = PhysicalAddress;

    fn add(self, rhs: usize) -> PhysicalAddress {
        PhysicalAddress::new_canonical(self.0.saturating_add(rhs))
    }
}

impl AddAssign<usize> for PhysicalAddress {
    fn add_assign(&mut self, rhs: usize) {
        *self = PhysicalAddress::new_canonical(self.0.saturating_add(rhs));
    }
}

impl Sub<usize> for PhysicalAddress {
    type Output = PhysicalAddress;

    fn sub(self, rhs: usize) -> PhysicalAddress {
        PhysicalAddress::new_canonical(self.0.saturating_sub(rhs))
    }
}

impl SubAssign<usize> for PhysicalAddress {
    fn sub_assign(&mut self, rhs: usize) {
        *self = PhysicalAddress::new_canonical(self.0.saturating_sub(rhs));
    }
}

impl From<PhysicalAddress> for usize {
    #[inline]
    fn from(virt_addr: PhysicalAddress) -> usize {
        virt_addr.0
    }
}


/// An area of physical memory.
#[derive(Copy, Clone, Debug, Default)]
#[repr(C)]
pub struct PhysicalMemoryArea {
    pub base_addr: PhysicalAddress,
    pub size_in_bytes: usize,
    pub typ: u32,
    pub acpi: u32,
}
impl PhysicalMemoryArea {
    pub fn new(
        paddr: PhysicalAddress,
        size_in_bytes: usize,
        typ: u32,
        acpi: u32,
    ) -> PhysicalMemoryArea {
        PhysicalMemoryArea {
            base_addr: paddr,
            size_in_bytes: size_in_bytes,
            typ: typ,
            acpi: acpi,
        }
    }
}


/// A `Frame` is a chunk of **physical** memory,
/// similar to how a `Page` is a chunk of **virtual** memory.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Frame {
    pub number: usize,
}
impl fmt::Debug for Frame {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Frame(paddr: {:#X})", self.start_address())
    }
}

impl Frame {
    /// Returns the `Frame` containing the given `PhysicalAddress`.
    pub fn containing_address(phys_addr: PhysicalAddress) -> Frame {
        Frame {
            number: phys_addr.value() / PAGE_SIZE,
        }
    }

    /// Returns the `PhysicalAddress` at the start of this `Frame`.
    pub fn start_address(&self) -> PhysicalAddress {
        PhysicalAddress::new_canonical(self.number * PAGE_SIZE)
    }
}

impl Add<usize> for Frame {
    type Output = Frame;

    fn add(self, rhs: usize) -> Frame {
        // cannot exceed max page number (which is also max frame number)
        Frame {
            number: core::cmp::min(MAX_PAGE_NUMBER, self.number.saturating_add(rhs)),
        }
    }
}

impl AddAssign<usize> for Frame {
    fn add_assign(&mut self, rhs: usize) {
        *self = Frame {
            number: core::cmp::min(MAX_PAGE_NUMBER, self.number.saturating_add(rhs)),
        };
    }
}

impl Sub<usize> for Frame {
    type Output = Frame;

    fn sub(self, rhs: usize) -> Frame {
        Frame {
            number: self.number.saturating_sub(rhs),
        }
    }
}

impl SubAssign<usize> for Frame {
    fn sub_assign(&mut self, rhs: usize) {
        *self = Frame {
            number: self.number.saturating_sub(rhs),
        };
    }
}

// Implementing these functions allow `Frame` to be in an `Iterator`.
impl Step for Frame {
    #[inline]
    fn steps_between(start: &Frame, end: &Frame) -> Option<usize> {
        Step::steps_between(&start.number, &end.number)
    }
    #[inline]
    fn replace_one(&mut self) -> Self {
        mem::replace(self, Frame { number: 1 })
    }
    #[inline]
    fn replace_zero(&mut self) -> Self {
        mem::replace(self, Frame { number: 0 })
    }
    #[inline]
    fn add_one(&self) -> Self {
        Add::add(*self, 1)
    }
    #[inline]
    fn sub_one(&self) -> Self {
        Sub::sub(*self, 1)
    }
    #[inline]
    fn add_usize(&self, n: usize) -> Option<Frame> {
        Some(*self + n)
    }
}


/// A range of `Frame`s that are contiguous in physical memory.
#[derive(Debug, Clone)]
pub struct FrameRange(RangeInclusive<Frame>);

impl FrameRange {
    /// Creates a new range of `Frame`s that spans from `start` to `end`,
    /// both inclusive bounds.
    pub fn new(start: Frame, end: Frame) -> FrameRange {
        FrameRange(RangeInclusive::new(start, end))
    }

    /// Creates a FrameRange that will always yield `None`.
    pub fn empty() -> FrameRange {
        FrameRange::new(Frame { number: 1 }, Frame { number: 0 })
    }

    /// A convenience method for creating a new `FrameRange`
    /// that spans all `Frame`s from the given physical address
    /// to an end bound based on the given size.
    pub fn from_phys_addr(starting_virt_addr: PhysicalAddress, size_in_bytes: usize) -> FrameRange {
        let start_frame = Frame::containing_address(starting_virt_addr);
        let end_frame = Frame::containing_address(starting_virt_addr + size_in_bytes - 1);
        FrameRange::new(start_frame, end_frame)
    }

    /// Returns the `PhysicalAddress` of the starting `Frame` in this `FrameRange`.
    pub fn start_address(&self) -> PhysicalAddress {
        self.0.start().start_address()
    }

    /// Returns the number of `Frame`s covered by this iterator.
    /// Use this instead of the Iterator trait's `count()` method.
    /// This is instant, because it doesn't need to iterate over each entry, unlike normal iterators.
    pub fn size_in_frames(&self) -> usize {
        // add 1 because it's an inclusive range
        self.0.end().number + 1 - self.0.start().number
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


/// A virtual memory page, which contains the index of the page
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Page {
    number: usize,
}
impl fmt::Debug for Page {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Page(vaddr: {:#X})", self.start_address())
    }
}

impl Page {
    /// Returns the `Page` that contains the given `VirtualAddress`.
    pub const fn containing_address(virt_addr: VirtualAddress) -> Page {
        Page {
            number: virt_addr.value() / PAGE_SIZE,
        }
    }

    /// Returns the `VirtualAddress` as the start of this `Page`.
    pub fn start_address(&self) -> VirtualAddress {
        // Cannot create VirtualAddress directly because the field is private
        VirtualAddress::new_canonical(self.number * PAGE_SIZE)
    }

    /// Returns the 9-bit part of this page's virtual address that is the index into the P4 page table entries list.
    pub fn p4_index(&self) -> usize {
        (self.number >> 27) & 0x1FF
    }

    /// Returns the 9-bit part of this page's virtual address that is the index into the P3 page table entries list.
    pub fn p3_index(&self) -> usize {
        (self.number >> 18) & 0x1FF
    }

    /// Returns the 9-bit part of this page's virtual address that is the index into the P2 page table entries list.
    pub fn p2_index(&self) -> usize {
        (self.number >> 9) & 0x1FF
    }

    /// Returns the 9-bit part of this page's virtual address that is the index into the P2 page table entries list.
    /// Using this returned `usize` value as an index into the P1 entries list will give you the final PTE,
    /// from which you can extract the mapped `Frame` (or its physical address) using `pointed_frame()`.
    pub fn p1_index(&self) -> usize {
        (self.number >> 0) & 0x1FF
    }
}

impl Add<usize> for Page {
    type Output = Page;

    fn add(self, rhs: usize) -> Page {
        // cannot exceed max page number
        Page {
            number: core::cmp::min(MAX_PAGE_NUMBER, self.number.saturating_add(rhs)),
        }
    }
}

impl AddAssign<usize> for Page {
    fn add_assign(&mut self, rhs: usize) {
        *self = Page {
            number: core::cmp::min(MAX_PAGE_NUMBER, self.number.saturating_add(rhs)),
        };
    }
}

impl Sub<usize> for Page {
    type Output = Page;

    fn sub(self, rhs: usize) -> Page {
        Page {
            number: self.number.saturating_sub(rhs),
        }
    }
}

impl SubAssign<usize> for Page {
    fn sub_assign(&mut self, rhs: usize) {
        *self = Page {
            number: self.number.saturating_sub(rhs),
        };
    }
}

// Implementing these functions allow `Page` to be in an `Iterator`.
impl Step for Page {
    #[inline]
    fn steps_between(start: &Page, end: &Page) -> Option<usize> {
        Step::steps_between(&start.number, &end.number)
    }
    #[inline]
    fn replace_one(&mut self) -> Self {
        mem::replace(self, Page { number: 1 })
    }
    #[inline]
    fn replace_zero(&mut self) -> Self {
        mem::replace(self, Page { number: 0 })
    }
    #[inline]
    fn add_one(&self) -> Self {
        Add::add(*self, 1)
    }
    #[inline]
    fn sub_one(&self) -> Self {
        Sub::sub(*self, 1)
    }
    #[inline]
    fn add_usize(&self, n: usize) -> Option<Page> {
        Some(*self + n)
    }
}



/// A range of `Page`s that are contiguous in virtual memory.
#[derive(Debug, Clone)]
pub struct PageRange(RangeInclusive<Page>);

impl PageRange {
    /// Creates a new range of `Page`s that spans from `start` to `end`,
    /// both inclusive bounds.
    pub fn new(start: Page, end: Page) -> PageRange {
        PageRange(RangeInclusive::new(start, end))
    }

    /// Creates a PageRange that will always yield `None`.
    pub fn empty() -> PageRange {
        PageRange::new(Page { number: 1 }, Page { number: 0 })
    }

    /// A convenience method for creating a new `PageRange`
    /// that spans all `Page`s from the given virtual address
    /// to an end bound based on the given size.
    pub fn from_virt_addr(starting_virt_addr: VirtualAddress, size_in_bytes: usize) -> PageRange {
        let start_page = Page::containing_address(starting_virt_addr);
        let end_page = Page::containing_address(starting_virt_addr + size_in_bytes - 1);
        PageRange::new(start_page, end_page)
    }

    /// Returns the `VirtualAddress` of the starting `Page`.
    pub fn start_address(&self) -> VirtualAddress {
        self.0.start().start_address()
    }

    /// Returns the size in number of `Page`s.
    /// Use this instead of the Iterator trait's `count()` method.
    /// This is instant, because it doesn't need to iterate over each `Page`, unlike normal iterators.
    pub fn size_in_pages(&self) -> usize {
        // add 1 because it's an inclusive range
        self.0.end().number + 1 - self.0.start().number
    }

    /// Returns the size in number of bytes.
    pub fn size_in_bytes(&self) -> usize {
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
/// It only contains three items, in which each item includes all sections that have identical flags:
/// * The `.text` section bounds cover all sections that are executable.
/// * The `.rodata` section bounds cover those that are read-only (.rodata, .gcc_except_table, .eh_frame).
/// * The `.data` section bounds cover those that are writable (.data, .bss).
pub struct AggregatedSectionMemoryBounds {
   pub text: SectionMemoryBounds,
   pub rodata: SectionMemoryBounds,
   pub data: SectionMemoryBounds,
}
