//! This crate contains common types used for memory mapping.

#![no_std]
#![feature(step_trait, const_trait_impl)]

mod allocator;
mod chunk;
mod chunk_range;

pub mod address;

pub use address::Address;
pub use chunk::Chunk;
pub use chunk_range::ChunkRange;
#[cfg(target_arch = "x86_64")]
pub use entryflags_x86_64::EntryFlags;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Debug)]
pub struct Virtual;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Debug)]
pub struct Physical;

mod private {
    pub trait Sealed {}
    impl Sealed for super::Virtual {}
    impl Sealed for super::Physical {}
}

// TODO: Constify functions
pub trait MemoryType:
    private::Sealed + Clone + Copy + PartialEq + Eq + PartialOrd + Ord + core::hash::Hash + Default
{
    const PREFIX: &'static str;

    const MIN_ADDRESS: usize;

    const MAX_ADDRESS: usize;

    fn is_canonical_address(address: usize) -> bool;

    fn canonicalize_address(address: usize) -> usize;

    type AllocatedChunksInner: ~const allocator::allocated::AllocatedChunksInner<MemoryType = Self>;

    type ChunkRangeWrapperInner: ~const allocator::chunk_range_wrapper::ChunkRangeWrapperInner;
}

impl const MemoryType for Virtual {
    const PREFIX: &'static str = "v";

    const MIN_ADDRESS: usize = usize::MIN;

    const MAX_ADDRESS: usize =
        Self::canonicalize_address(kernel_config::memory::MAX_VIRTUAL_ADDRESS);

    #[inline]
    fn is_canonical_address(address: usize) -> bool {
        matches!(get_bits(address, 47..64), 0 | 0b1_1111_1111_1111_1111)
    }

    #[inline]
    fn canonicalize_address(address: usize) -> usize {
        // match virt_addr.get_bit(47) {
        //     false => virt_addr.set_bits(48..64, 0),
        //     true =>  virt_addr.set_bits(48..64, 0xffff),
        // };

        // The below code is semantically equivalent to the above, but it works in const functions.
        ((address << 16) as isize >> 16) as usize
    }

    type AllocatedChunksInner = allocator::allocated::AllocatedChunksVirtual;

    type ChunkRangeWrapperInner = allocator::chunk_range_wrapper::ChunkRangeWrapperVirtual;
}

impl const MemoryType for Physical {
    const PREFIX: &'static str = "p";

    const MIN_ADDRESS: usize = usize::MIN;

    const MAX_ADDRESS: usize = Self::canonicalize_address(usize::MAX);

    #[inline]
    fn is_canonical_address(address: usize) -> bool {
        get_bits(address, 52..64) == 0
    }

    #[inline]
    fn canonicalize_address(address: usize) -> usize {
        address & 0x000F_FFFF_FFFF_FFFF
    }

    type AllocatedChunksInner = allocator::allocated::AllocatedChunksPhysical;

    type ChunkRangeWrapperInner = allocator::chunk_range_wrapper::ChunkRangeWrapperPhysical;
}

/// Taken from the `bit_field` crate, but specialised to [`core::ops::Range`] to allow for the
/// function to be used in a const context.
#[inline]
const fn get_bits(value: usize, range: core::ops::Range<usize>) -> usize {
    const BIT_LENGTH: usize = ::core::mem::size_of::<usize>() * 8;

    assert!(range.start < BIT_LENGTH);
    assert!(range.end <= BIT_LENGTH);
    assert!(range.start < range.end);

    // shift away high bits
    let bits = value << (BIT_LENGTH - range.end) >> (BIT_LENGTH - range.end);

    // shift away low bits
    bits >> range.start
}

/// The address bounds and mapping flags of a section's memory region.
#[derive(Debug)]
pub struct SectionMemoryBounds {
    /// The starting virtual address and physical address.
    pub start: (Address<Virtual>, Address<Physical>),
    /// The ending virtual address and physical address.
    pub end: (Address<Virtual>, Address<Physical>),
    /// The page table entry flags that should be used for mapping this section.
    pub flags: EntryFlags,
}

/// The address bounds and flags of the initial kernel sections that need mapping.
///
/// Individual sections in the kernel's ELF image are combined here according to their flags,
/// as described below, but some are kept separate for the sake of correctness or ease of use.
///
/// It contains three main items, in which each item includes all sections that have identical flags:
/// * The `text` section bounds cover all sections that are executable.
/// * The `rodata` section bounds cover those that are read-only (.rodata, .gcc_except_table, .eh_frame).
///   * The `rodata` section also includes thread-local storage (TLS) areas (.tdata, .tbss) if they exist,
///     because they can be mapped using the same page table flags.
/// * The `data` section bounds cover those that are writable (.data, .bss).
///
/// It also contains:
/// * The `page_table` section bounds cover the initial page table's top-level (root) P4 frame.
/// * The `stack` section bounds cover the initial stack, which are maintained separately.
#[derive(Debug)]
pub struct AggregatedSectionMemoryBounds {
    pub text: SectionMemoryBounds,
    pub rodata: SectionMemoryBounds,
    pub data: SectionMemoryBounds,
    pub page_table: SectionMemoryBounds,
    pub stack: SectionMemoryBounds,
}
