// Copyright 2016 Philipp Oppermann. See the README.md
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

pub use super::super::EntryFlags;
use super::super::{Frame};
use multiboot2;
use xmas_elf;
use PhysicalAddress;
use bit_field::BitField;
use kernel_config::memory::PAGE_SHIFT;


/// A page table entry, which is a `u64` value under the hood.
/// It contains a physical frame address and entry flag access bits.
#[repr(transparent)]
pub struct Entry(u64);

impl Entry {
    pub fn is_unused(&self) -> bool {
        self.0 == 0
    }

    pub fn set_unused(&mut self) {
        self.0 = 0;
    }

    pub fn flags(&self) -> EntryFlags {
        EntryFlags::from_bits_truncate(self.0)
    }

    pub fn pointed_frame(&self) -> Option<Frame> {
        if self.flags().contains(EntryFlags::PRESENT) {
            let mut frame_paddr = self.0 as usize;
            frame_paddr.set_bits(0 .. (PAGE_SHIFT as u8), 0);
            Some(Frame::containing_address(PhysicalAddress::new_canonical(frame_paddr)))
        } else {
            None
        }
    }

    pub fn set(&mut self, frame: Frame, flags: EntryFlags) {
        self.0 = (frame.start_address().value() as u64) | flags.bits();
    }

    // we use this to force explicit copying rather than deriving Copy/Clone
    pub fn copy(&self) -> Entry {
        Entry(self.0)
    }

    pub fn value(&self) -> u64 {
        self.0
    }
}

pub trait FlagOperator {
    fn is_writable(&self) -> bool;

    fn is_executable(&self) -> bool;

    fn from_multiboot2_section_flags(section: &multiboot2::ElfSection) -> EntryFlags;

    fn from_elf_section_flags(elf_flags: u64) -> EntryFlags;

    fn from_elf_program_flags(prog_flags: xmas_elf::program::Flags) -> EntryFlags;
}

impl FlagOperator for EntryFlags {
    /// Returns true if these flags have the `WRITABLE` bit set.
    fn is_writable(&self) -> bool {
        self.intersects(EntryFlags::WRITABLE)
    }

    /// Returns true if these flags are executable, 
    /// which means that the `NO_EXECUTE` bit on x86 is *not* set.
    fn is_executable(&self) -> bool {
        !self.intersects(EntryFlags::NO_EXECUTE)
    }

    fn from_multiboot2_section_flags(section: &multiboot2::ElfSection) -> EntryFlags {
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

    fn from_elf_section_flags(elf_flags: u64) -> EntryFlags {
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

    fn from_elf_program_flags(prog_flags: xmas_elf::program::Flags) -> EntryFlags {
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
