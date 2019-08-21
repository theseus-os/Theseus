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
#[macro_use] extern crate bitflags;
extern crate heap_irq_safe;
#[macro_use] extern crate derive_more;
extern crate bit_field;
extern crate type_name;
extern crate uefi;
#[cfg(target_arch = "x86_64")]
extern crate mmu_x86;
#[cfg(target_arch = "aarch64")]
extern crate mmu_arm;
extern crate entry_flags_oper;

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


mod area_frame_allocator;
mod stack_allocator;
pub mod paging;

pub use self::area_frame_allocator::AreaFrameAllocator;
pub use self::paging::*;
pub use self::stack_allocator::{StackAllocator, Stack};

#[cfg(target_arch = "x86_64")]
use mmu_x86::{KERNEL_OFFSET_BITS_START, KERNEL_OFFSET_PREFIX, set_new_p4, get_p4_address, flush, flush_all};
#[cfg(target_arch = "aarch64")]
use mmu_arm::{KERNEL_OFFSET_BITS_START, KERNEL_OFFSET_PREFIX, set_new_p4, get_p4_address, flush, flush_all};

pub use entry_flags_oper::EntryFlagsOper;
#[cfg(target_arch = "x86_64")]
pub use mmu_x86::EntryFlags;
#[cfg(target_arch = "aarch64")]
pub use mmu_arm::EntryFlags;

use core::{
    ops::{RangeInclusive, Deref, DerefMut},
    iter::Step,
    mem,
};
use multiboot2::BootInformation;
use spin::Once;
use irq_safety::MutexIrqSafe;
use alloc::vec::Vec;
use alloc::sync::Arc;
use kernel_config::memory::{PAGE_SIZE, MAX_PAGE_NUMBER, KERNEL_HEAP_START, KERNEL_HEAP_INITIAL_SIZE, KERNEL_STACK_ALLOCATOR_BOTTOM, KERNEL_STACK_ALLOCATOR_TOP_ADDR, ENTRIES_PER_PAGE_TABLE};
use bit_field::BitField;
use uefi::prelude::*;
use uefi::table::boot::{MemoryDescriptor, MemoryType};

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
        match virt_addr.get_bits(KERNEL_OFFSET_BITS_START..64) {
            0 | KERNEL_OFFSET_PREFIX => Ok(VirtualAddress(virt_addr)),
            _ => Err("VirtualAddress bits 48-63 must be a sign-extension of bit 47"),
        }
    }

/// Creates a new `VirtualAddress` that is guaranteed to be canonical
    /// by forcing the upper bits (64:48] to be sign-extended from bit 47.
    pub fn new_canonical(mut virt_addr: usize) -> VirtualAddress {
        match virt_addr.get_bit(KERNEL_OFFSET_BITS_START) {
            false => virt_addr.set_bits(48..64, 0),
            true  => virt_addr.set_bits(48..64, 0xffff),
        };
        VirtualAddress(virt_addr)
    }

    /// Creates a VirtualAddress with the value 0.
    pub const fn zero() -> VirtualAddress {
        VirtualAddress(0)
    }

    /// Returns the underlying `usize` value for this `VirtualAddress`.
    #[inline]
    pub fn value(&self) -> usize {
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

impl core::fmt::Pointer for VirtualAddress {
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
    AddAssign, SubAssign, MulAssign, DivAssign, RemAssign, ShrAssign, ShlAssign
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



/// The memory management info and address space of the kernel
pub static KERNEL_MMI: Once<Arc<MutexIrqSafe<MemoryManagementInfo>>> = Once::new();

/// returns a cloned reference to the kernel's `MemoryManagementInfo`, if initialized.
/// If not, it returns None.
pub fn get_kernel_mmi_ref() -> Option<Arc<MutexIrqSafe<MemoryManagementInfo>>> {
    KERNEL_MMI.try().cloned()
}


/// The one and only frame allocator, a singleton. 
pub static FRAME_ALLOCATOR: Once<MutexIrqSafe<AreaFrameAllocator>> = Once::new();

/// Convenience method for allocating a new Frame.
pub fn allocate_frame() -> Option<Frame> {
    FRAME_ALLOCATOR.try().and_then(|fa| fa.lock().allocate_frame())
}


/// Convenience method for allocating several contiguous Frames.
pub fn allocate_frames(num_frames: usize) -> Option<FrameRange> {
    FRAME_ALLOCATOR.try().and_then(|fa| fa.lock().allocate_frames(num_frames))
}

/// An Arc reference to a `MemoryManagementInfo` struct.
pub type MmiRef = Arc<MutexIrqSafe<MemoryManagementInfo>>;


/// This holds all the information for a `Task`'s memory mappings and address space
/// (this is basically the equivalent of Linux's mm_struct)
#[derive(Debug)]
pub struct MemoryManagementInfo {
    /// the PageTable that should be switched to when this Task is switched to.
    pub page_table: PageTable,
    
    /// the list of virtual memory areas mapped currently in this Task's address space
    pub vmas: Vec<VirtualMemoryArea>,

    /// a list of additional virtual-mapped Pages that have the same lifetime as this MMI
    /// and are thus owned by this MMI, but is not all-inclusive (e.g., Stacks are excluded).
    /// Someday this could likely replace the vmas list, but VMAs offer sub-page granularity for now.
    pub extra_mapped_pages: Vec<MappedPages>,

    /// the task's stack allocator, which is initialized with a range of Pages from which to allocate.
    pub stack_allocator: stack_allocator::StackAllocator,  // TODO: this shouldn't be public, once we move spawn_userspace code into this module
}

impl MemoryManagementInfo {

    /// Allocates a new stack in the currently-running Task's address space.
    /// Also, this adds the newly-allocated stack to this struct's `vmas` vector. 
    /// Whether this is a kernelspace or userspace stack is determined by how this MMI's stack_allocator was initialized.
    /// 
    /// # Important Note
    /// You cannot call this to allocate a stack in a different `MemoryManagementInfo`/`PageTable` than the one you're currently running. 
    /// It will only work for allocating a stack in the currently-running MMI.
    pub fn alloc_stack(&mut self, size_in_pages: usize) -> Option<Stack> {
        let &mut MemoryManagementInfo { ref mut page_table, ref mut vmas, ref mut stack_allocator, .. } = self;
    
        if let Some( (stack, stack_vma) ) = FRAME_ALLOCATOR.try().and_then(|fa| stack_allocator.alloc_stack(page_table, fa.lock().deref_mut(), size_in_pages)) {
            vmas.push(stack_vma);
            Some(stack)
        }
        else {
            error!("MemoryManagementInfo::alloc_stack: failed to allocate stack of {} pages!", size_in_pages);
            None
        }
    }
}

pub fn get_current_p4() -> Frame {
    Frame::containing_address(PhysicalAddress::new_canonical(get_p4_address()))
}

/// A convenience function that creates a new memory mapping by allocating frames that are contiguous in physical memory.
/// Returns a tuple containing the new `MappedPages` and the starting PhysicalAddress of the first frame,
/// which is a convenient way to get the physical address without walking the page tables.
/// 
/// # Locking / Deadlock
/// Currently, this function acquires the lock on the `FRAME_ALLOCATOR` and the kernel's `MemoryManagementInfo` instance.
/// Thus, the caller should ensure that the locks on those two variables are not held when invoking this function.
pub fn create_contiguous_mapping(size_in_bytes: usize, flags: EntryFlags) -> Result<(MappedPages, PhysicalAddress), &'static str> {
    let allocated_pages = allocate_pages_by_bytes(size_in_bytes).ok_or("e1000::create_contiguous_mapping(): couldn't allocate pages!")?;

    let kernel_mmi_ref = get_kernel_mmi_ref().ok_or("create_contiguous_mapping(): KERNEL_MMI was not yet initialized!")?;
    let mut kernel_mmi = kernel_mmi_ref.lock();

    let mut frame_allocator = FRAME_ALLOCATOR.try()
        .ok_or("create_contiguous_mapping(): couldnt get FRAME_ALLOCATOR")?
        .lock();
    let frames = frame_allocator.allocate_frames(allocated_pages.size_in_pages())
        .ok_or("create_contiguous_mapping(): couldnt allocate a new frame")?;
    let starting_phys_addr = frames.start_address();
    let mp = kernel_mmi.page_table.map_allocated_pages_to(allocated_pages, frames, flags, frame_allocator.deref_mut())?;
    Ok((mp, starting_phys_addr))
}


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



/// A region of virtual memory that is mapped into a [`Task`](../task/struct.Task.html)'s address space
#[derive(Debug, Default, Clone, PartialEq)]
pub struct VirtualMemoryArea {
    start: VirtualAddress,
    size: usize,
    flags: EntryFlags,
    desc: &'static str,
}
use core::fmt;
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
    number: usize,
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

