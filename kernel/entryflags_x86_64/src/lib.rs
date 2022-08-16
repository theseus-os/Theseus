//! This crate defines the structure of page table entry flags on x86_64.

#![no_std]

#[macro_use] extern crate bitflags;
#[macro_use] extern crate static_assertions;
extern crate multiboot2;
extern crate xmas_elf;


bitflags! {
    /// Page table entry flags on the x86_64 architecture. 
    /// 
    /// The designation of bits in each `PageTableEntry` is as such:
    /// * Bits `[0:8]` (inclusive) are reserved by hardware for access flags.
    /// * Bits `[9:11]` (inclusive) are available for custom OS usage.
    /// * Bits `[12:51]` (inclusive) are reserved by hardware to hold the physical frame address.
    /// * Bits `[52:62]` (inclusive) are available for custom OS usage.
    /// * Bit  `63` is reserved by hardware for access flags (noexec).
    /// 
    pub struct EntryFlags: u64 {
        /// If set, this page is currently "present" in memory. 
        /// If not set, this page is not in memory, e.g., not mapped, paged to disk, etc.
        const PRESENT           = 1 <<  0;
        /// If set, writes to this page are allowed.
        /// If not set, this page is read-only.
        const WRITABLE          = 1 <<  1;
        /// If set, userspace (ring 3) can access this page.
        /// If not set, only kernelspace (ring 0) can access this page. 
        const USER_ACCESSIBLE   = 1 <<  2;
        /// If set, writes to this page go directly through the cache to memory. 
        const WRITE_THROUGH     = 1 <<  3;
        /// If set, this page's content is never cached, neither for read nor writes. 
        const NO_CACHE          = 1 <<  4;
        /// The hardware will set this bit when the page is accessed.
        const ACCESSED          = 1 <<  5;
        /// The hardware will set this bit when the page has been written to.
        const DIRTY             = 1 <<  6;
        /// Set this bit if this page table entry represents a "huge" page. 
        /// This bit may be used as follows:
        /// * For a P4-level PTE, it must be not set. 
        /// * If set for a P3-level PTE, it means this PTE maps a 1GiB huge page.
        /// * If set for a P2-level PTE, it means this PTE maps a 1MiB huge page.
        /// * For a P1-level PTE, it must be not set. 
        const HUGE_PAGE         = 1 <<  7;
        /// Set this bit to indicate that this page is mapped across all address spaces 
        /// (all root page tables) and doesn't need to be flushed out of the TLB 
        /// when switching to another page table. 
        const GLOBAL            = 0 <<  8; // 1 <<  8; // Currently disabling GLOBAL bit.

        /// Set this bit to indicate that the frame pointed to by this page table entry
        /// is owned **exclusively** by that page table entry.
        /// Currently, in Theseus, we only set the `EXCLUSIVE` bit for P1-level PTEs
        /// that we **know** are bijective (1-to-1 virtual-to-physical) mappings. 
        /// If this bit is set, the pointed frame will be safely deallocated
        /// once this page table entry is unmapped. 
        const EXCLUSIVE         = 1 <<  9;

        /// Set this bit to forbid execution of the mapped page.
        /// In other words, if you want the page to be executable, do NOT set this bit. 
        const NO_EXECUTE        = 1 << 63;
    }
}

// Ensure that we never expose reserved bits [12:51] as part of the `EntryFlags` interface.
const_assert_eq!(EntryFlags::all().bits() & 0x000_FFFFFFFFFF_000, 0);

impl EntryFlags {
    /// Returns a new, all-zero `EntryFlags` with no bits enabled.
    /// 
    /// This is a `const` version of `Default::default`.
    pub const fn zero() -> EntryFlags {
        EntryFlags::from_bits_truncate(0)
    }

    /// Returns `true` if the page the entry points to is a huge page.
    pub const fn is_huge(&self) -> bool {
        self.intersects(EntryFlags::HUGE_PAGE)
    }

    /// Copies this new `EntryFlags` object and sets the huge page bit.
    pub const fn into_huge(&self) -> EntryFlags {
        EntryFlags::from_bits_truncate(
            self.bits() | EntryFlags::HUGE_PAGE.bits()
        )
    }

    /// Returns `true` if the page is writable.
    pub const fn is_writable(&self) -> bool {
        self.intersects(EntryFlags::WRITABLE)
    }

    /// Copies this new `EntryFlags` object and sets the writable bit.
    pub const fn into_writable(&self) -> EntryFlags {
        EntryFlags::from_bits_truncate(
            self.bits() | EntryFlags::WRITABLE.bits()
        )
    }

    /// Returns `true` if these flags are executable.
    pub const fn is_executable(&self) -> bool {
        // On x86_64, this means that the `NO_EXECUTE` bit is *not* set.
        !self.intersects(EntryFlags::NO_EXECUTE)
    }

    /// Returns `true` if these flags are exclusive. 
    pub const fn is_exclusive(&self) -> bool {
        self.intersects(EntryFlags::EXCLUSIVE)
    }

    /// Copies this `EntryFlags` into a new one with the exclusive bit cleared.
    pub const fn into_non_exclusive(&self) -> EntryFlags {
        // This is a const way to write:  `self | EntryFlags::WRITABLE`
        EntryFlags::from_bits_truncate(
            self.bits() & !EntryFlags::EXCLUSIVE.bits()
        )
    }

    /// Copies this `EntryFlags` into a new one with the exclusive bit cleared.
    pub const fn into_exclusive(&self) -> EntryFlags {
        // This is a const way to write:  `self | EntryFlags::WRITABLE`
        EntryFlags::from_bits_truncate(
            self.bits() | EntryFlags::EXCLUSIVE.bits()
        )
    }

    /// Gets flags according to the properties of a section from multiboot2.
    pub fn from_multiboot2_section_flags(section: &multiboot2::ElfSection) -> EntryFlags {
        use multiboot2::ElfSectionFlags;

        let mut flags = EntryFlags::empty();

        if section.flags().contains(ElfSectionFlags::ALLOCATED) {
            // section is loaded to memory
            flags |= EntryFlags::PRESENT;
        }
        if section.flags().contains(ElfSectionFlags::WRITABLE) {
            flags |= EntryFlags::WRITABLE;
        }
        if !section.flags().contains(ElfSectionFlags::EXECUTABLE) {
            flags |= EntryFlags::NO_EXECUTE;
        }

        flags
    }

    /// Gets flags according to the properties of a section from elf flags.
    pub fn from_elf_section_flags(elf_flags: u64) -> EntryFlags {
        use xmas_elf::sections::{SHF_ALLOC, SHF_EXECINSTR, SHF_WRITE};

        let mut flags = EntryFlags::empty();

        if elf_flags & SHF_ALLOC == SHF_ALLOC {
            // section is loaded to memory
            flags |= EntryFlags::PRESENT;
        }
        if elf_flags & SHF_WRITE == SHF_WRITE {
            flags |= EntryFlags::WRITABLE;
        }
        if elf_flags & SHF_EXECINSTR == 0 {
            // only mark no execute if the execute flag isn't 1
            flags |= EntryFlags::NO_EXECUTE;
        }

        flags
    }

    /// Gets flags according to the properties of a program. 
    pub fn from_elf_program_flags(prog_flags: xmas_elf::program::Flags) -> EntryFlags {
        let mut flags = EntryFlags::empty();

        if prog_flags.is_read() {
            // section is loaded to memory
            flags |= EntryFlags::PRESENT;
        }
        if prog_flags.is_write() {
            flags |= EntryFlags::WRITABLE;
        }
        if !prog_flags.is_execute() {
            // only mark no execute if the execute flag isn't 1
            flags |= EntryFlags::NO_EXECUTE;
        }

        flags
    }
}

impl Default for EntryFlags {
    fn default() -> Self {
        Self::zero()
    }
}
