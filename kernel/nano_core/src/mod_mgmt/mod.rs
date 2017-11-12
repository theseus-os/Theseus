use xmas_elf::ElfFile;
use xmas_elf::program::Type;
use core::slice;
use core::ptr;
use collections::Vec;
use memory::{VirtualMemoryArea, VirtualAddress, PhysicalAddress, EntryFlags};



// Can also try this crate: https://crates.io/crates/goblin

/// the minimum size that an Elf file must be, 52 bytes.
const ELF_HEADER_SIZE: usize = 52;


pub struct ElfProgramSection {
    /// the VirtualMemoryAddress that will represent the virtual mapping of this Program section.
    /// Provides starting virtual address, size in memory, mapping flags, and a text description.
    pub vma: VirtualMemoryArea,
    /// the offset of this section into the file.
    /// This plus the physical address of the Elf file is the physical address of this Program section.
    pub offset: usize,
}


/// parses an elf executable file as a slice of bytes starting at the given `start_addr`.
pub fn parse_elf_executable(start_addr: *const u8, size: usize) -> Result<(Vec<ElfProgramSection>, VirtualAddress), ()> {
    debug!("Parsing Elf: start_addr {:#x}, size {:#x}({})", start_addr as usize, size, size);
    if start_addr.is_null() || size < ELF_HEADER_SIZE { 
        return Err(()); 
    }

    // SAFE: safe enough, checked for null 
    let byte_slice = unsafe { slice::from_raw_parts(start_addr, size) };
    let elf_file = ElfFile::new(byte_slice).unwrap();
    debug!("Elf File: {:?}", elf_file);

    let mut prog_sects: Vec<ElfProgramSection> = Vec::new();
    for prog in elf_file.program_iter() {
        debug!("   prog: {}", prog);
        let typ = prog.get_type();
        if typ != Ok(Type::Load) {
            warn!("Program type in ELF file wasn't LOAD, {}", prog);
            return Err(());
        }
        let flags = EntryFlags::from_elf_program_flags(prog.flags());
        use memory::*;
        if !flags.contains(PRESENT) {
            warn!("Program flags in ELF file wasn't Read, so EntryFlags wasn't PRESENT!!\n{}", prog);
            return Err(());
        }
        // TODO: how to get name of program section?
        // could infer it based on perms, like .text or .data
        prog_sects.push(ElfProgramSection {
            vma: VirtualMemoryArea::new(prog.virtual_addr() as VirtualAddress, prog.mem_size() as usize, flags, "test_name"),
            offset: prog.offset() as usize,
        });
    }

    let entry_point = elf_file.header.pt2.entry_point() as VirtualAddress;

    Ok((prog_sects, entry_point))
}



// code snippets for analyzing sections in ELF, not programs
// {
//     for sec in elf_file.section_iter() {
//         debug!("Elf Section: {:?}", sec);
//         debug!("             name {:?} type {:?}", sec.get_name(&elf_file), sec.get_type());
//         if sec.get_type().unwrap() == ShType::ProgBits {
//             // map all of the PROGBITS sections
//             trace!("Found ProgBits section: {:?}", sec.get_name(&elf_file));
//             let entry_flags = EntryFlags::from_elf_section_flags(sec.flags()); 
//             sections.push(VirtualMemoryArea::new(sec.address() as usize, sec.size() as usize, entry_flags, sec.get_name(&elf_file).unwrap()));
//         }
//     }

//     let entry_point: usize = elf_file.header.pt2.entry_point() as usize;


//     debug!("Entry_point: {:#x}, new VMAs: {:?}", entry_point, sections);
//     Ok((entry_point, sections))
// }