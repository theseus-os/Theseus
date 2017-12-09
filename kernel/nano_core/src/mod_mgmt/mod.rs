use xmas_elf::ElfFile;
use xmas_elf::program::Type;
use xmas_elf::sections::ShType;
use core::slice;
use core::ptr;
use alloc::Vec;
use memory::{VirtualMemoryArea, VirtualAddress, PhysicalAddress, EntryFlags};
use rustc_demangle::try_demangle;

mod metadata;
use self::metadata::*;

// Can also try this crate: https://crates.io/crates/goblin

// /// the minimum size that an Elf file must be, 52 bytes.
// const ELF_HEADER_SIZE: usize = 52;


pub struct ElfProgramSection {
    /// the VirtualMemoryAddress that will represent the virtual mapping of this Program section.
    /// Provides starting virtual address, size in memory, mapping flags, and a text description.
    pub vma: VirtualMemoryArea,
    /// the offset of this section into the file.
    /// This plus the physical address of the Elf file is the physical address of this Program section.
    pub offset: usize,
}


/// parses an elf executable file as a slice of bytes starting at the given `start_addr`, 
/// which must be a VirtualAddress currently mapped into the kernel's address space.
pub fn parse_elf_executable(start_addr: VirtualAddress, size: usize) -> Result<(Vec<ElfProgramSection>, VirtualAddress), ()> {
    debug!("Parsing Elf executable: start_addr {:#x}, size {:#x}({})", start_addr, size, size);
    let start_addr = start_addr as *const u8;
    if start_addr.is_null() {
        return Err(()); 
    }

    // SAFE: safe enough, checked for null 
    let byte_slice = unsafe { slice::from_raw_parts(start_addr, size) };
    let elf_file = ElfFile::new(byte_slice).unwrap();
    // debug!("Elf File: {:?}", elf_file);

    let mut prog_sects: Vec<ElfProgramSection> = Vec::new();
    for prog in elf_file.program_iter() {
        // debug!("   prog: {}", prog);
        let typ = prog.get_type();
        if typ != Ok(Type::Load) {
            warn!("Program type in ELF file wasn't LOAD, {}", prog);
            return Err(());
        }
        let flags = EntryFlags::from_elf_program_flags(prog.flags());
        use memory::*;
        if !flags.contains(EntryFlags::PRESENT) {
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


pub fn parse_elf_kernel_crate(start_addr: VirtualAddress, size: usize) -> Result<(), &'static str> {
    debug!("Parsing Elf kernel crate: start_addr {:#x}, size {:#x}({})", start_addr as usize, size, size);
    let start_addr = start_addr as *const u8;
    if start_addr.is_null() {
        return Err("start_addr for parse_elf_kernel_crate is null!");
    }

    // SAFE: safe enough, checked for null 
    let byte_slice = unsafe { slice::from_raw_parts(start_addr, size) };
    // debug!("BYTE SLICE: {:?}", byte_slice);
    let elf_file = try!(ElfFile::new(byte_slice)); // returns Err(&str) if ELF parse fails
    // debug!("Elf File: {:?}\n\n", elf_file);
    {
        for sec in elf_file.section_iter() {
            let sec_type = sec.get_type(); 
            if sec_type.is_err() { continue; }

            match sec_type.unwrap() {
                ShType::ProgBits => {
                    // the PROGBITS sections (including .text sections) are what we care about
                    let size = sec.size(); 
                    if size == 0 { continue; }

                    const text_prefix: &'static str = ".text.";
                    let text_prefix_end: usize = text_prefix.len();
                    if let Ok(name) = sec.get_name(&elf_file) {
                        if name.starts_with(text_prefix) {
                            if let Some(name) = name.get(text_prefix_end..) {
                                let demangled = try_demangle(name);
                                trace!("Found .text section: {}", sec);
                                trace!("Found .text section: {:?} {:?}, size={:#x}, start_addr={:#x}", 
                                        name, demangled, sec.size(), sec.address());
                                let entry_flags = EntryFlags::from_elf_section_flags(sec.flags()); 
                            }

                        }
                    }
                    else {
                        warn!("parse_elf_kernel_crate: couldn't get section name!");
                        continue;
                    }
                }
                ShType::Rel => {
                    trace!("Found Rel section: {:?}", sec);
                    trace!("      Relf data: {:?}", sec.get_data(&elf_file));
                }
                ShType::Rela => {
                    trace!("Found Rela section: {:?}", sec);
                    trace!("      Rela data: {:?}", sec.get_data(&elf_file));
                }
                _ => { 
                    debug!("Skipping unneeded Elf Section: {:?}", sec);
                    debug!("         name {:?} type {:?}", sec.get_name(&elf_file), sec.get_type());
                    continue; 
                }
            }
        }
    }

    Err("not doing anything yet!")
}

