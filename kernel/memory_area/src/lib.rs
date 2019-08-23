//! This crate implements the virtual memory subsystem for Theseus,
//! which is fairly robust and provides a unification between 
//! arbitrarily mapped sections of memory and Rust's lifetime system. 
//! Originally based on Phil Opp's blog_os. 

#![no_std]
#![feature(asm)]
#![feature(ptr_internals)]
#![feature(core_intrinsics)]
#![feature(unboxed_closures)]
#![feature(step_trait, range_is_empty)]

extern crate spin;
extern crate multiboot2;
extern crate alloc;
#[macro_use] extern crate lazy_static;
#[macro_use] extern crate log;
extern crate irq_safety;
extern crate kernel_config;
extern crate atomic_linked_list;
extern crate xmas_elf;
extern crate heap_irq_safe;
#[macro_use] extern crate derive_more;
extern crate bit_field;
extern crate type_name;
extern crate page_table_x86;

mod page;
mod address;

pub use page::*;
pub use address::*;

/// Just like Rust's `try!()` macro, 
/// but forgets the given `obj`s to prevent them from being dropped,
/// as they would normally be upon return of an Error using `try!()`.
/// This must come BEFORE the below modules in order for them to be able to use it.
#[macro_export]
macro_rules! try_forget {
    ($expr:expr, $($obj:expr),*) => (match $expr {
        Ok(val) => val,
        Err(err) => {
            $(
                core::mem::forget($obj);
            )*
            return Err(err);
        }
    });
}

use core::{
    ops::{RangeInclusive, Deref, DerefMut},
    iter::Step,
    mem,
    fmt
};
use spin::Once;
use irq_safety::MutexIrqSafe;
use alloc::vec::Vec;
use alloc::sync::Arc;
use kernel_config::memory::{PAGE_SIZE, MAX_PAGE_NUMBER, KERNEL_HEAP_START, KERNEL_HEAP_INITIAL_SIZE, KERNEL_STACK_ALLOCATOR_BOTTOM, KERNEL_STACK_ALLOCATOR_TOP_ADDR, KERNEL_HEAP_P4_INDEX, KERNEL_STACK_P4_INDEX, KERNEL_TEXT_P4_INDEX, KERNEL_OFFSET};
use bit_field::BitField;
use page_table_x86::EntryFlags;


/// An area of physical memory. 
#[derive(Copy, Clone, Debug, Default)]
#[repr(C)]
pub struct PhysicalMemoryArea {
    pub base_addr: PhysicalAddress,
    pub size_in_bytes: usize,
    pub typ: u32,
    pub acpi: u32
}
impl PhysicalMemoryArea {
    pub fn new(paddr: PhysicalAddress, size_in_bytes: usize, typ: u32, acpi: u32) -> PhysicalMemoryArea {
        PhysicalMemoryArea {
            base_addr: paddr,
            size_in_bytes: size_in_bytes,
            typ: typ,
            acpi: acpi,
        }
    }
}



static BROADCAST_TLB_SHOOTDOWN_FUNC: Once<fn(Vec<VirtualAddress>)> = Once::new();

/// Set the function callback that will be invoked every time a TLB shootdown is necessary,
/// i.e., during page table remapping and unmapping operations.
pub fn set_broadcast_tlb_shootdown_cb(func: fn(Vec<VirtualAddress>)) {
    BROADCAST_TLB_SHOOTDOWN_FUNC.call_once(|| func);
}

/// A `Frame` is a chunk of **physical** memory, 
/// similar to how a `Page` is a chunk of **virtual** memory. 
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Frame {
    // Wenqiu: set as pub because it is used by memory::paging::mapper;
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
        Frame { number: phys_addr.value() / PAGE_SIZE }
    }

    /// Returns the `PhysicalAddress` at the start of this `Frame`.
    pub fn start_address(&self) -> PhysicalAddress {
        PhysicalAddress::new_canonical(self.number * PAGE_SIZE)
    }
}

use core::ops::{Add, AddAssign, Sub, SubAssign};
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
        Frame { number: self.number.saturating_sub(rhs) }
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
        let end   = core::cmp::max(self.0.end(),   &frame_to_include);
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


pub trait FrameAllocator {
    fn allocate_frame(&mut self) -> Option<Frame>;
    fn allocate_frames(&mut self, num_frames: usize) -> Option<FrameRange>;
    fn deallocate_frame(&mut self, frame: Frame);
    /// Call this when a heap is set up, and the `alloc` types can be used.
    fn alloc_ready(&mut self);
}

/// A region of virtual memory that is mapped into a [`Task`](../task/struct.Task.html)'s address space
#[derive(Debug, Default, Clone, PartialEq)]
pub struct VirtualMemoryArea {
    start: VirtualAddress,
    size: usize,
    flags: EntryFlags,
    desc: &'static str,
}

impl fmt::Display for VirtualMemoryArea {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "start: {:#X}, size: {:#X}, flags: {:#X}, desc: {}", 
                  self.start, self.size, self.flags, self.desc
        )
    }
}


impl VirtualMemoryArea {
    pub fn new(start: VirtualAddress, size: usize, flags: EntryFlags, desc: &'static str) -> Self {
        VirtualMemoryArea {
            start: start,
            size: size,
            flags: flags,
            desc: desc,
        }
    }

    pub fn start_address(&self) -> VirtualAddress {
        self.start
    }

    pub fn size(&self) -> usize {
        self.size
    }

    pub fn flags(&self) -> EntryFlags {
        self.flags
    }

    pub fn desc(&self) -> &'static str {
        self.desc
    }

    /// Get an iterator that covers all the pages in this VirtualMemoryArea
    pub fn pages(&self) -> PageRange {

        // check that the end_page won't be invalid
        if (self.start.value() + self.size) < 1 {
            return PageRange::empty();
        }
        
        let start_page = Page::containing_address(self.start);
        let end_page = Page::containing_address(self.start + self.size - 1);
        PageRange::new(start_page, end_page)
    }
}

