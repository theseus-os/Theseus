//! This crate defines the entryflags of page table on x86_64.  
//! Definitions in this crate should be exported from `memory_x86_64` for upper-level crates.

#![no_std]

#[macro_use] extern crate bitflags;
extern crate multiboot2;
extern crate xmas_elf;


bitflags! {
    /// Entry access flag bits.
    #[derive(Default)]
    pub struct EntryFlags: u64 {
        const PRESENT           = 1 << 0;
        const WRITABLE          = 1 << 1;
        const USER_ACCESSIBLE   = 1 << 2;
        const WRITE_THROUGH     = 1 << 3;
        const NO_CACHE          = 1 << 4;
        const ACCESSED          = 1 << 5;
        const DIRTY             = 1 << 6;
        const HUGE_PAGE         = 1 << 7;
        // const GLOBAL            = 1 << 8;
        const GLOBAL            = 0; // disabling because VirtualBox doesn't like it
        const NO_EXECUTE        = 1 << 63;
    }

}

impl EntryFlags {
    /// Returns ture if the page the entry points to is a huge page.
    /// For x86_64, it means the flags contain a `HUGE_PAGE` bit.
    pub fn is_huge(&self) -> bool {
        self.contains(EntryFlags::HUGE_PAGE)
    }

    /// Returns the bits that must be set for an accessible page.
    /// For x86_64, the `PRESENT` bit should be set.
    pub fn present() -> EntryFlags {
        EntryFlags::PRESENT
    }

    /// Returns the flags of an accessiable writable page.
    /// For x86_64 the `PRESENT` and `WRITABLE` bits should be set.
    pub fn writable_page() -> EntryFlags {
        EntryFlags::present() | EntryFlags::WRITABLE
    }

    /// Returns true if the page is accessible and is not huge.
    pub fn is_regular_page(&self) -> bool {
        self.contains(EntryFlags::PRESENT) && !self.contains(EntryFlags::HUGE_PAGE)
    }

    /// Sets the entryflags as writable and accessible and returns it.
    /// For x86_64 the `PRESENT` and `WRITABLE` bits should be set.
    pub fn as_writable_page(self) -> EntryFlags {
        self | EntryFlags::writable_page()
    }

    /// Returns true if the page is writable.
    /// For x86_64 it means the flags contain `WRITABLE`.
    pub fn is_writable(&self) -> bool {
        self.intersects(EntryFlags::WRITABLE)
    }

    /// Returns true if these flags are executable,
    /// which means that the `NO_EXECUTE` bit on x86_64 is *not* set.
    pub fn is_executable(&self) -> bool {
        !self.intersects(EntryFlags::NO_EXECUTE)
    }

    /// Gets flags according to the properties of a section from multiboot2.
    pub fn from_multiboot2_section_flags(section: &multiboot2::ElfSection) -> EntryFlags {
        use multiboot2::ElfSectionFlags;

        let mut flags = EntryFlags::empty();

        if section.flags().contains(ElfSectionFlags::ALLOCATED) {
            // section is loaded to memory
            flags = flags | EntryFlags::PRESENT;
        }
        if section.flags().contains(ElfSectionFlags::WRITABLE) {
            flags = flags | EntryFlags::WRITABLE;
        }
        if !section.flags().contains(ElfSectionFlags::EXECUTABLE) {
            flags = flags | EntryFlags::NO_EXECUTE;
        }

        flags
    }

    /// Gets flags according to the properties of a section from elf flags.
    pub fn from_elf_section_flags(elf_flags: u64) -> EntryFlags {
        use xmas_elf::sections::{SHF_ALLOC, SHF_EXECINSTR, SHF_WRITE};

        let mut flags = EntryFlags::empty();

        if elf_flags & SHF_ALLOC == SHF_ALLOC {
            // section is loaded to memory
            flags = flags | EntryFlags::PRESENT;
        }
        if elf_flags & SHF_WRITE == SHF_WRITE {
            flags = flags | EntryFlags::WRITABLE;
        }
        if elf_flags & SHF_EXECINSTR == 0 {
            // only mark no execute if the execute flag isn't 1
            flags = flags | EntryFlags::NO_EXECUTE;
        }

        flags
    }

    /// Gets flags according to the properties of a program. 
    pub fn from_elf_program_flags(prog_flags: xmas_elf::program::Flags) -> EntryFlags {
        let mut flags = EntryFlags::empty();

        if prog_flags.is_read() {
            // section is loaded to memory
            flags = flags | EntryFlags::PRESENT;
        }
        if prog_flags.is_write() {
            flags = flags | EntryFlags::WRITABLE;
        }
        if !prog_flags.is_execute() {
            // only mark no execute if the execute flag isn't 1
            flags = flags | EntryFlags::NO_EXECUTE;
        }

        flags
    }
}

