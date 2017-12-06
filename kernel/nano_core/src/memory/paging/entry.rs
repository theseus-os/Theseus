// Copyright 2016 Philipp Oppermann. See the README.md
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use memory::Frame;
use multiboot2;
use xmas_elf;

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
        if self.flags().contains(PRESENT) {
            Some(Frame::containing_address(self.0 as usize & 0x000fffff_fffff000))
        } else {
            None
        }
    }

    pub fn set(&mut self, frame: Frame, flags: EntryFlags) {
        assert!(frame.start_address() & !0x000fffff_fffff000 == 0);
        self.0 = (frame.start_address() as u64) | flags.bits();
    }

    // we use this to force explicit copying rather than deriving Copy/Clone
    pub fn copy(&self) -> Entry {
        Entry(self.0)
    }
}

bitflags! {
    #[derive(Default)]
    pub flags EntryFlags: u64 {
        const PRESENT =         1 << 0,
        const WRITABLE =        1 << 1,
        const USER_ACCESSIBLE = 1 << 2,
        const WRITE_THROUGH =   1 << 3,
        const NO_CACHE =        1 << 4,
        const ACCESSED =        1 << 5,
        const DIRTY =           1 << 6,
        const HUGE_PAGE =       1 << 7,
        const GLOBAL =          1 << 8,
        const NO_EXECUTE =      1 << 63,
    }
}

impl EntryFlags {
    pub fn from_multiboot2_section_flags(section: &multiboot2::ElfSection) -> EntryFlags {
        use multiboot2::{ELF_SECTION_ALLOCATED, ELF_SECTION_WRITABLE, ELF_SECTION_EXECUTABLE};

        let mut flags = EntryFlags::empty();

        if section.flags().contains(ELF_SECTION_ALLOCATED) {
            // section is loaded to memory
            flags = flags | PRESENT;
        }
        if section.flags().contains(ELF_SECTION_WRITABLE) {
            flags = flags | WRITABLE;
        }
        if !section.flags().contains(ELF_SECTION_EXECUTABLE) {
            flags = flags | NO_EXECUTE;
        }

        flags
    }

    // pub fn from_elf_section_flags(elf_flags: u64) -> EntryFlags {
    //     use xmas_elf::sections::{SHF_WRITE, SHF_ALLOC, SHF_EXECINSTR};
        
    //     let mut flags = EntryFlags::empty();

    //     if elf_flags & SHF_ALLOC == SHF_ALLOC {
    //         // section is loaded to memory
    //         flags = flags | PRESENT;
    //     }
    //     if elf_flags & SHF_WRITE == SHF_WRITE {
    //         flags = flags | WRITABLE;
    //     }
    //     if elf_flags & SHF_EXECINSTR == 0 {
    //         // only mark no execute if the execute flag isn't 1
    //         flags = flags | NO_EXECUTE;
    //     }

    //     flags
    // }

     pub fn from_elf_program_flags(prog_flags: xmas_elf::program::Flags) -> EntryFlags {
        use xmas_elf::program::{FLAG_R, FLAG_W, FLAG_X};
        
        let mut flags = EntryFlags::empty();

        // stupid ass hack because Flags is a private struct
        // debug!("CHECKING PROG_FLAGS: flags: {:#x} {:?}", prog_flags, prog_flags);
        let prog_flags: u32 = unsafe { ::core::mem::transmute(prog_flags) };
        // debug!("CHECKING PROG_FLAGS: flags: {:#x} {:?}", prog_flags, prog_flags);

        if prog_flags & FLAG_R == FLAG_R {
            // section is loaded to memory
            flags = flags | PRESENT;
        }
        if prog_flags & FLAG_W == FLAG_W {
            flags = flags | WRITABLE;
        }
        if prog_flags & FLAG_X == 0 {
            // only mark no execute if the execute flag isn't 1
            flags = flags | NO_EXECUTE;
        }

        flags
    }
}
