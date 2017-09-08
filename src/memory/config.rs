//! WARNING: DO NOT USE ANY ADDRESS THAT MAPS TO THE SAME P4 ENTRY AS THE ONE
//! USED FOR THE RECURSIVE PAGE TABLE ENTRY (CURRENTLY 510). 
//! Currently, that would be any address that starts with 0xFFFF_FF0*_****_****,
//! so do not use that virtual address range for anything!!

//! Current P4 (top-level page table) mappings:
//! * 511: kernel text sections
//! * 510: recursive mapping to top of P4
//! * 509: kernel heap
//! * 508: kernel stacks
//! * 507: userspace stacks
//! * 506 down to 0:  available for user processes


/// 64-bit architecture results in 8 bytes per address.
pub const BYTES_PER_ADDR: usize = 8;

/// The lower 12 bits of a virtual address correspond to the P1 page frame offset. 
pub const PAGE_SHIFT: usize = 12;
/// page size is 4096, 4KiB pages.
pub const PAGE_SIZE: usize = (1 << PAGE_SHIFT);
/// returns true if an address is page-aligned
pub fn address_is_page_aligned(addr: usize) -> bool {
    addr % PAGE_SIZE == 0
}

/// Value: 0. Shift the Page number (not the address!) by this to get the P1 index.
pub const P1_INDEX_SHIFT: usize = 0;
/// Value: 9. Shift the Page number (not the address!) by this to get the P2 index.
pub const P2_INDEX_SHIFT: usize = P1_INDEX_SHIFT + 9;
/// Value: 18. Shift the Page number (not the address!) by this to get the P3 index.
pub const P3_INDEX_SHIFT: usize = P2_INDEX_SHIFT + 9;
/// Value: 27. Shift the Page number (not the address!) by this to get the P4 index.
pub const P4_INDEX_SHIFT: usize = P3_INDEX_SHIFT + 9;

/// Value: 512 GiB.
pub const ADDRESSABILITY_PER_P4_ENTRY: usize = (1 << (PAGE_SHIFT + P4_INDEX_SHIFT));

pub const MAX_VIRTUAL_ADDRESS: usize = 0xFFFF_FFFF_FFFF_FFFF;

pub const TEMPORARY_PAGE_VIRT_ADDR: usize = MAX_VIRTUAL_ADDRESS;

/// This is 32, because Rust currently supports default initialization of 
/// primitive arrays that are up to 32 elements long.
pub const MAX_MEMORY_AREAS: usize = 32;

/// Value: 512. 
pub const ENTRIES_PER_PAGE_TABLE: usize = PAGE_SIZE / BYTES_PER_ADDR;
/// Value: 511. The 511th entry is used for kernel text sections
pub const KERNEL_TEXT_P4_INDEX: usize = ENTRIES_PER_PAGE_TABLE - 1;
/// Value: 510. The 510th entry is used for the recursive P4 mapping.
pub const RECURSIVE_PAGE_TABLE_INDEX: usize = ENTRIES_PER_PAGE_TABLE - 2;
/// Value: 509. The 509th entry is used for the kernel heap
pub const KERNEL_HEAP_P4_INDEX: usize = ENTRIES_PER_PAGE_TABLE - 3;
/// Value: 508. The 508th entry is used for all kernel stacks
pub const KERNEL_STACK_P4_INDEX: usize = ENTRIES_PER_PAGE_TABLE - 4;
/// Value: 507. The 507th entry is used for all userspace stacks
pub const USER_STACK_P4_INDEX: usize = ENTRIES_PER_PAGE_TABLE - 5;


pub const MAX_PAGE_NUMBER: usize = MAX_VIRTUAL_ADDRESS / PAGE_SIZE;


/// the virtual address where the kernel is mapped to: 0xFFFFFFFF80000000.
/// i.e., the linear offset between physical memory and kernel memory
/// so the VGA buffer will be mapped from 0xb8000 to 0xFFFFFFFF800b8000.
pub const KERNEL_OFFSET: usize = 0xFFFFFFFF80000000;

/// higher-half heap gets 512 GB address range starting at the 509th P4 entry,
/// which is the slot right below the recursive P4 entry (510)
/// actual value: 0o177777_775_000_000_000_0000, or 0xFFFF_FE80_0000_0000
pub const KERNEL_HEAP_START: usize = 0xFFFF_0000_0000_0000 | (KERNEL_HEAP_P4_INDEX << (P4_INDEX_SHIFT + PAGE_SHIFT));
pub const KERNEL_HEAP_INITIAL_SIZE: usize = 1 * 1024 * 1024; // 1 MiB
/// the kernel heap gets the whole 509th P4 entry.
pub const KERNEL_HEAP_MAX_SIZE: usize = ADDRESSABILITY_PER_P4_ENTRY;


/// the kernel stack allocator gets the 508th P4 entry of addressability. 
pub const KERNEL_STACK_ALLOCATOR_BOTTOM: usize = 0xFFFF_0000_0000_0000 | (KERNEL_STACK_P4_INDEX << (P4_INDEX_SHIFT + PAGE_SHIFT));
/// the highest actually usuable address in the kernel stack allocator
pub const KERNEL_STACK_ALLOCATOR_TOP_ADDR: usize = KERNEL_STACK_ALLOCATOR_BOTTOM + ADDRESSABILITY_PER_P4_ENTRY - BYTES_PER_ADDR;


/// the userspace stack allocators (one per userspace task) each get the 507th P4 entry of addressability. 
pub const USER_STACK_ALLOCATOR_BOTTOM: usize = 0xFFFF_0000_0000_0000 | (USER_STACK_P4_INDEX << (P4_INDEX_SHIFT + PAGE_SHIFT));
/// the highest actually usuable address in each userspace stack allocator
pub const USER_STACK_ALLOCATOR_TOP_ADDR: usize = USER_STACK_ALLOCATOR_BOTTOM + ADDRESSABILITY_PER_P4_ENTRY - BYTES_PER_ADDR;

