//! This crate defines the structure of page table entry flags on x86_64.

#![no_std]

#[macro_use] extern crate bitflags;
extern crate multiboot2;
extern crate xmas_elf;


bitflags! {
    /// Page table entry flags.
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
    /// Returns true if the page the entry points to is a huge page.
    pub fn is_huge(&self) -> bool {
        self.intersects(EntryFlags::HUGE_PAGE)
    }

    /// Copies this new `EntryFlags` object and sets the huge page flag.
    pub fn into_huge(self) -> EntryFlags {
        self | EntryFlags::HUGE_PAGE
    }

    /// Returns true if the page is writable.
    pub fn is_writable(&self) -> bool {
        self.intersects(EntryFlags::WRITABLE)
    }

    /// Copies this new `EntryFlags` object and sets the writable flag.
    pub fn into_writable(self) -> EntryFlags {
        self | EntryFlags::WRITABLE
    }

    /// Returns true if these flags are executable.
    pub fn is_executable(&self) -> bool {
        // On x86_64, this means that the `NO_EXECUTE` bit is *not* set.
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

