//! The basic virtual memory map that Theseus assumes.
//!
//! Current P4 (top-level page table) mappings:
//! * 511: kernel text sections.
//! * 510: recursive mapping for accessing the current P4 root page table frame.
//! * 509: kernel heap.
//! * 508: recursive mapping for accessing the P4 root page table frame
//!        of an upcoming new page table.
//! * 507 down to 0: available for general usage.

/// 64-bit architecture results in 8 bytes per address.
pub const BYTES_PER_ADDR: usize = core::mem::size_of::<usize>();

/// The lower 12 bits of a virtual address correspond to the P1 page frame offset. 
pub const PAGE_SHIFT: usize = 12;
/// Page size is 4096 bytes, 4KiB pages.
pub const PAGE_SIZE: usize = 1 << PAGE_SHIFT;

/// Value: 0. Shift the Page number (not the address!) by this to get the P1 index.
pub const P1_INDEX_SHIFT: usize = 0;
/// Value: 9. Shift the Page number (not the address!) by this to get the P2 index.
pub const P2_INDEX_SHIFT: usize = P1_INDEX_SHIFT + 9;
/// Value: 18. Shift the Page number (not the address!) by this to get the P3 index.
pub const P3_INDEX_SHIFT: usize = P2_INDEX_SHIFT + 9;
/// Value: 27. Shift the Page number (not the address!) by this to get the P4 index.
pub const P4_INDEX_SHIFT: usize = P3_INDEX_SHIFT + 9;

/// Value: 512 GiB.
pub const ADDRESSABILITY_PER_P4_ENTRY: usize = 1 << (PAGE_SHIFT + P4_INDEX_SHIFT);

pub const MAX_VIRTUAL_ADDRESS: usize = usize::MAX;

pub const TEMPORARY_PAGE_VIRT_ADDR: usize = MAX_VIRTUAL_ADDRESS;

/// Value: 512.
pub const ENTRIES_PER_PAGE_TABLE: usize = PAGE_SIZE / BYTES_PER_ADDR;
/// Value: 511. The 511th entry is used for kernel text sections
pub const KERNEL_TEXT_P4_INDEX: usize = ENTRIES_PER_PAGE_TABLE - 1;
/// Value: 510. The 510th entry is used to recursively map the current P4 root page table frame
//              such that it can be accessed and modified just like any other level of page table.
pub const RECURSIVE_P4_INDEX: usize = ENTRIES_PER_PAGE_TABLE - 2;
/// Value: 509. The 509th entry is used for the kernel heap
pub const KERNEL_HEAP_P4_INDEX: usize = ENTRIES_PER_PAGE_TABLE - 3;
/// Value: 508. The 508th entry is used to temporarily recursively map the P4 root page table frame
//              of an upcoming (new) page table such that it can be accessed and modified.
pub const UPCOMING_PAGE_TABLE_RECURSIVE_P4_INDEX: usize = ENTRIES_PER_PAGE_TABLE - 4;


pub const MAX_PAGE_NUMBER: usize = MAX_VIRTUAL_ADDRESS / PAGE_SIZE;

/// The size in pages of each kernel stack. 
/// If it's too small, complex kernel functions will overflow, causing a page fault / double fault.
#[cfg(not(debug_assertions))]
pub const KERNEL_STACK_SIZE_IN_PAGES: usize = 16;
#[cfg(debug_assertions)]
pub const KERNEL_STACK_SIZE_IN_PAGES: usize = 32; // debug builds require more stack space.

/// The virtual address where the initial kernel (the nano_core) is mapped to.
/// Actual value: 0xFFFFFFFF80000000.
/// i.e., the linear offset between physical memory and kernel memory.
/// So, for example, the VGA buffer will be mapped from 0xb8000 to 0xFFFFFFFF800b8000.
/// This is -2GiB from the end of the 64-bit address space.
pub const KERNEL_OFFSET: usize = 0xFFFF_FFFF_8000_0000;



/// The kernel text region is where we load kernel modules. 
/// It starts at the 511th P4 entry and goes up until the KERNEL_OFFSET,
/// which is where the nano_core itself starts. 
/// Actual value: 0o177777_777_000_000_000_0000, or 0xFFFF_FF80_0000_0000
pub const KERNEL_TEXT_START: usize = 0xFFFF_0000_0000_0000 | (KERNEL_TEXT_P4_INDEX << (P4_INDEX_SHIFT + PAGE_SHIFT));
/// The size in bytes, not in pages.
pub const KERNEL_TEXT_MAX_SIZE: usize = ADDRESSABILITY_PER_P4_ENTRY - (2 * 1024 * 1024 * 1024); // because the KERNEL_OFFSET starts at -2GiB


/// The higher-half heap gets the 512GB address range starting at the 509th P4 entry,
/// which is the slot right below the recursive P4 entry (510).
/// Actual value: 0o177777_775_000_000_000_0000, or 0xFFFF_FE80_0000_0000
pub const KERNEL_HEAP_START: usize = 0xFFFF_0000_0000_0000 | (KERNEL_HEAP_P4_INDEX << (P4_INDEX_SHIFT + PAGE_SHIFT));
#[cfg(not(debug_assertions))]
pub const KERNEL_HEAP_INITIAL_SIZE: usize = 64 * 1024 * 1024; // 64 MiB
#[cfg(debug_assertions)]
pub const KERNEL_HEAP_INITIAL_SIZE: usize = 256 * 1024 * 1024; // 256 MiB, debug builds require more heap space.
/// the kernel heap gets the whole 509th P4 entry.
pub const KERNEL_HEAP_MAX_SIZE: usize = ADDRESSABILITY_PER_P4_ENTRY;

/// The system (page allocator) must not use addresses at or above this address.
pub const UPCOMING_PAGE_TABLE_RECURSIVE_MEMORY_START: usize = 0xFFFF_0000_0000_0000 | (UPCOMING_PAGE_TABLE_RECURSIVE_P4_INDEX << (P4_INDEX_SHIFT + PAGE_SHIFT));
