#![no_std]

extern crate x86_64;
extern crate kernel_config;
#[macro_use] extern crate bitflags;
extern crate xmas_elf;
extern crate multiboot2;
extern crate entry_flags_oper;

use x86_64::{instructions::tlb, registers::control_regs};
pub use kernel_config::memory::{KERNEL_OFFSET};
use entry_flags_oper::EntryFlagsOper;

bitflags! {
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

impl EntryFlagsOper<EntryFlags> for EntryFlags {
    /// Returns ture if the page the entry points to is a huge page. 
    /// Which means the flags contains a HUGE_PAGE bit
    fn is_huge(&self) -> bool {
        self.contains(EntryFlags::HUGE_PAGE)
    }

    /// The default flags of an accessible page. 
    /// For every accessiable page the PRESENT bit should be set
    fn default_flags() -> EntryFlags {
        EntryFlags::PRESENT
    }

    /// return the flags of a writable page excluding the default bits
    fn writable() -> EntryFlags {
        EntryFlags::WRITABLE
    }

    /// The flags of a writable page. 
    /// For every writable page the PRESENT and WRITABLE bits should be set
    fn rw_flags() -> EntryFlags {
        EntryFlags::default_flags() | EntryFlags::WRITABLE
    }

    /// Returns true if the page is accessiable and is not huge
    fn is_page(&self) -> bool {
        self.contains(EntryFlags::PRESENT) && !self.contains(EntryFlags::HUGE_PAGE)
    }

    /// Set the page the entry points to as writable.
    /// Set the PRESENT and WRITABLE bits of the flags
    fn set_writable(&self) -> EntryFlags {
        self.clone() | EntryFlags::rw_flags()
    }

    /// Returns true if these flags have the `WRITABLE` bit set.
    fn is_writable(&self) -> bool {
        self.intersects(EntryFlags::WRITABLE)
    }

    /// Returns true if these flags are executable, 
    /// which means that the `NO_EXECUTE` bit on x86 is *not* set.
    fn is_executable(&self) -> bool {
        !self.intersects(EntryFlags::NO_EXECUTE)
    }

}

impl EntryFlags {
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

    pub fn from_elf_section_flags(elf_flags: u64) -> EntryFlags {
        use xmas_elf::sections::{SHF_WRITE, SHF_ALLOC, SHF_EXECINSTR};
        
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


/// Set the new P4 table address. 
/// Switch to the new page table p4 points to
pub fn set_new_p4(p4: u64) {
    unsafe {
        control_regs::cr3_write(x86_64::PhysicalAddress(p4));
    }
}


/// Returns the current top-level page table frame, e.g., cr3 on x86
// pub fn get_p4_address() -> Frame {
//     
// }
pub fn get_p4_address() -> usize {
    control_regs::cr3().0 as usize
}

/// Flush the virtual address translation buffer of the specific address
pub fn flush(address:usize) {
    tlb::flush(x86_64::VirtualAddress(address));
}

/// Flush all the virtual address translation buffer
pub fn flush_all() {
    tlb::flush_all();
}