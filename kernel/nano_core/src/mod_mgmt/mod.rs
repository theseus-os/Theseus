use xmas_elf::ElfFile;
use xmas_elf::program::Type;
use xmas_elf::sections::{SectionData, ShType};
use core::slice;
use core::ptr;
use core::ops::DerefMut;
use spin::Mutex;
use alloc::{Vec, String};
use alloc::string::ToString;
use memory::{VirtualMemoryArea, VirtualAddress, PhysicalAddress, EntryFlags, ActivePageTable, FRAME_ALLOCATOR};
use rustc_demangle::{demangle, try_demangle};

mod metadata;
use self::metadata::*;

// Can also try this crate: https://crates.io/crates/goblin
// ELF RESOURCE: http://www.cirosantilli.com/elf-hello-world


lazy_static! {
    static ref MODULE_TREE: Mutex<Vec<LoadedCrate>> = Mutex::new(Vec::new());
}


pub struct ElfProgramSegment {
    /// the VirtualMemoryAddress that will represent the virtual mapping of this Program segment.
    /// Provides starting virtual address, size in memory, mapping flags, and a text description.
    pub vma: VirtualMemoryArea,
    /// the offset of this segment into the file.
    /// This plus the physical address of the Elf file is the physical address of this Program segment.
    pub offset: usize,
}


/// parses an elf executable file as a slice of bytes starting at the given `start_addr`,
/// which must be a VirtualAddress currently mapped into the kernel's address space.
pub fn parse_elf_executable(start_addr: VirtualAddress, size: usize) -> Result<(Vec<ElfProgramSegment>, VirtualAddress), &'static str> {
    debug!("Parsing Elf executable: start_addr {:#x}, size {:#x}({})", start_addr, size, size);
    let start_addr = start_addr as *const u8;
    if start_addr.is_null() {
        return Err("start_addr was null!");
    }

    // SAFE: checked for null
    let byte_slice = unsafe { slice::from_raw_parts(start_addr, size) };
    let elf_file = try!(ElfFile::new(byte_slice));
    // debug!("Elf File: {:?}", elf_file);

    let mut prog_sects: Vec<ElfProgramSegment> = Vec::new();
    for prog in elf_file.program_iter() {
        // debug!("   prog: {}", prog);
        let typ = prog.get_type();
        if typ != Ok(Type::Load) {
            warn!("Program type in ELF file wasn't LOAD, {}", prog);
            return Err("Program type in ELF file wasn't LOAD");
        }
        let flags = EntryFlags::from_elf_program_flags(prog.flags());
        use memory::*;
        if !flags.contains(EntryFlags::PRESENT) {
            warn!("Program flags in ELF file wasn't Read, so EntryFlags wasn't PRESENT!! {}", prog);
            return Err("Program flags in ELF file wasn't Read, so EntryFlags wasn't PRESENT!");
        }
        // TODO: how to get name of program section?
        // could infer it based on perms, like .text or .data
        prog_sects.push(ElfProgramSegment {
            vma: VirtualMemoryArea::new(prog.virtual_addr() as VirtualAddress, prog.mem_size() as usize, flags, "test_name"),
            offset: prog.offset() as usize,
        });
    }

    let entry_point = elf_file.header.pt2.entry_point() as VirtualAddress;

    Ok((prog_sects, entry_point))
}



// pub struct ElfTextSection {
//     /// The full demangled name of this text section
//     pub demangled_name: String,
//     // /// The offset where this section exists within the ElfFile.
//     // pub offset: usize,
//     /// the slice including the actual data of this text section
//     pub data: [u8]
//     /// The size in bytes of this text section
//     pub size: usize,
//     /// The flags to be used when mapping this section into memory
//     pub flags: EntryFlags,
// }


pub fn parse_elf_kernel_crate(start_addr: VirtualAddress, size: usize, module_name: &str, active_table: &mut ActivePageTable)
    -> Result<LoadedCrate, &'static str>
{
    debug!("Parsing Elf kernel crate: {:?}, start_addr {:#x}, size {:#x}({})", module_name, start_addr as usize, size, size);
    let start_addr = start_addr as *const u8;
    if start_addr.is_null() {
        error!("start_addr for parse_elf_kernel_crate is null!");
        return Err("start_addr for parse_elf_kernel_crate is null!");
    }
    if !module_name.starts_with("__k_") {
        error!("Error parsing crate: {}, name must start with __k_.", module_name);
        return Err("module_name didn't start with __k_");
    }

    // SAFE: checked for null
    let byte_slice = unsafe { slice::from_raw_parts(start_addr, size) };
    // debug!("BYTE SLICE: {:?}", byte_slice);
    let elf_file = try!(ElfFile::new(byte_slice)); // returns Err(&str) if ELF parse fails

    let total_size: usize = {
        let mut total = 0;
        for s in elf_file.section_iter() {
            if let Ok(ShType::ProgBits) = s.get_type() {
                 total += s.size() as usize;
            }
        }
        total
    };

    // we need to remap or copy each section content to a new address,
    // and then create metadata structures to represent it
    use memory::virtual_address_allocator::allocate_pages_by_bytes;
    let allocated_pages = try!(allocate_pages_by_bytes(total_size));
    let mut section_vaddr = {
        // let total_size = text_sections.iter().fold(0, |acc, &sec| acc + sec.size);
        use memory::FRAME_ALLOCATOR;
        let mut frame_allocator = FRAME_ALLOCATOR.try().unwrap().lock();

        // right now we're just simply copying small sections to the new memory
        // so we have to map those pages to real (randomly chosen) frames first
        for p in 0..allocated_pages.num_pages {
            // text sections use flags: present and executable (not NX)
            // because we're copying bytes to the newly allocated pages, we need to make them writaable too
            active_table.map(allocated_pages.start + p, EntryFlags::PRESENT | EntryFlags::WRITABLE, frame_allocator.deref_mut());
        }
        allocated_pages.start.start_address()
    };


    let mut text_sections: Vec<LoadedTextSection> = Vec::new();

    {
        for sec in elf_file.section_iter() {
            let sec_type = sec.get_type();
            if sec_type.is_err() { continue; }

            match sec_type.unwrap() {
                // the PROGBITS sections are the bulk of what we care about, i.e., .text sections
                ShType::ProgBits => {
                    // skip null section and any empty sections
                    let sec_size = sec.size() as usize;
                    if sec_size == 0 { continue; }

                    const text_prefix: &'static str = ".text.";
                    let text_prefix_end: usize = text_prefix.len();
                    if let Ok(name) = sec.get_name(&elf_file) {
                        if name.starts_with(text_prefix) {
                            if let Some(name) = name.get(text_prefix_end..) {
                                let demangled: String = demangle(name).to_string();
                                trace!("Found .text section: {:?} {:?}, size={:#x}",
                                        name, demangled, sec_size);
                                // let entry_flags = EntryFlags::from_elf_section_flags(sec.flags());

                                if let Ok(SectionData::Undefined(sec_data)) = sec.get_data(&elf_file) {
                                    // SAFE: we have allocated the pages containing section_vaddr and mapped them above
                                    let dest: &mut [u8] = unsafe { slice::from_raw_parts_mut(section_vaddr as *mut u8, sec_size) };
                                    dest.copy_from_slice(sec_data);

                                    text_sections.push(LoadedTextSection{
                                        symbol: demangled.clone(),
                                        abs_symbol: demangled.clone(),
                                        hash: None,
                                        virt_addr: section_vaddr,
                                        size: sec_size,
                                    });

                                    section_vaddr += sec_size; // advance the pointer for the next section
                                }
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
                    // debug!("         name {:?} type {:?}", sec.get_name(&elf_file), sec.get_type());
                    continue;
                }
            }
        }
    }

    // since we initially mapped the pages as writable, we need to remap them as non-writable
    // because text section pages shouldn't be writable
    for p in 0..allocated_pages.num_pages {
        // text sections use flags: present and executable (not NX)
        // because we're copying bytes to the newly allocated pages, we need to make them writaable too
        active_table.remap(allocated_pages.start + p, EntryFlags::PRESENT);
    }

    Ok(LoadedCrate {
        crate_name: String::from(module_name.get(3..).unwrap()),
        text_sections: text_sections,
        owned_pages: allocated_pages,
    })

}
