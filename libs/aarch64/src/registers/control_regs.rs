//! Functions to read and write control registers.
//! See AMD64 Vol. 2 Section 3.1.1

#![allow(missing_docs)]

use {VirtualAddress, PhysicalAddress};

bitflags! {
    /// Provides operating-mode controls and some processor-feature controls.
    pub flags Cr0: usize {
        const ENABLE_PAGING = 1 << 31,
        const CACHE_DISABLE = 1 << 30,
        const NOT_WRITE_THROUGH = 1 << 29,
        const ALIGNMENT_MASK = 1 << 18,
        const WRITE_PROTECT = 1 << 16,
        const NUMERIC_ERROR = 1 << 5,
        const EXTENSION_TYPE = 1 << 4,
        const TASK_SWITCHED = 1 << 3,
        const EMULATE_COPROCESSOR = 1 << 2,
        const MONITOR_COPROCESSOR = 1 << 1,
        const PROTECTED_MODE = 1 << 0,
    }
}

bitflags! {
    /// This register contains additional controls for various operating-mode features.
    #[allow(missing_docs)]
    pub flags Cr4: usize {
        const ENABLE_SMAP = 1 << 21,
        const ENABLE_SMEP = 1 << 20,
        const ENABLE_OS_XSAVE = 1 << 18,
        const ENABLE_PCID = 1 << 17,
        const ENABLE_SMX = 1 << 14,
        const ENABLE_VMX = 1 << 13,
        const UNMASKED_SSE = 1 << 10,
        const ENABLE_SSE = 1 << 9,
        const ENABLE_PPMC = 1 << 8,
        const ENABLE_GLOBAL_PAGES = 1 << 7,
        const ENABLE_MACHINE_CHECK = 1 << 6,
        const ENABLE_PAE = 1 << 5,
        const ENABLE_PSE = 1 << 4,
        const DEBUGGING_EXTENSIONS = 1 << 3,
        const TIME_STAMP_DISABLE = 1 << 2,
        const VIRTUAL_INTERRUPTS = 1 << 1,
        const ENABLE_VME = 1 << 0,
    }
}

/// Read CR0
pub fn cr0() -> Cr0 {
    // TODO
    Cr0::from_bits_truncate(0)
}

/// Write CR0.
///
/// # Safety
/// Changing the CR0 register is unsafe, because e.g. disabling paging would violate memory safety.
pub unsafe fn cr0_write(_val: Cr0) {
}

/// Update CR0.
///
/// # Safety
/// Changing the CR0 register is unsafe, because e.g. disabling paging would violate memory safety.
pub unsafe fn cr0_update<F>(f: F)
    where F: FnOnce(&mut Cr0)
{
    let mut value = cr0();
    f(&mut value);
    cr0_write(value);
}

/// Contains page-fault virtual address.
pub fn cr2() -> VirtualAddress {
    VirtualAddress(0)
}

/// Contains page-table root pointer.
pub fn cr3() -> PhysicalAddress {
    PhysicalAddress(0)
}

/// Switch page-table PML4 pointer (level 4 page table).
///
/// # Safety
/// Changing the level 4 page table is unsafe, because it's possible to violate memory safety by
/// changing the page mapping.
pub unsafe fn cr3_write(_val: PhysicalAddress) {
}

/// Contains various flags to control operations in protected mode.
pub fn cr4() -> Cr4 {
    Cr4::from_bits_truncate(0)
}

/// Write cr4.
///
/// # Safety
/// It's not clear if it's always memory safe to change the CR4 register.
pub unsafe fn cr4_write(_val: Cr4) {
}
