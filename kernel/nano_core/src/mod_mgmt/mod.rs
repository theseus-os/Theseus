use xmas_elf::ElfFile;
use xmas_elf::program::Type;
use xmas_elf::sections::{SectionHeader, SectionData, ShType};
use xmas_elf::sections::{SHF_WRITE, SHF_ALLOC, SHF_EXECINSTR};
use core::slice;
use core::ptr;
use core::ops::DerefMut;
use spin::Mutex;
use alloc::{Vec, String};
use alloc::string::ToString;
use memory::{VirtualMemoryArea, VirtualAddress, PhysicalAddress, EntryFlags, ActivePageTable, FRAME_ALLOCATOR};
use memory::virtual_address_allocator::OwnedContiguousPages;
use rustc_demangle::{demangle, try_demangle};

pub mod metadata;
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
        error!("parse_elf_kernel_crate(): start_addr is null!");
        return Err("start_addr for parse_elf_kernel_crate is null!");
    }
    if !module_name.starts_with("__k_") {
        error!("parse_elf_kernel_crate(): error parsing crate: {}, name must start with __k_.", module_name);
        return Err("module_name didn't start with __k_");
    }

    // SAFE: checked for null
    let byte_slice = unsafe { slice::from_raw_parts(start_addr, size) };
    // debug!("BYTE SLICE: {:?}", byte_slice);
    let elf_file = try!(ElfFile::new(byte_slice)); // returns Err(&str) if ELF parse fails

    // For us to properly load the ELF file, it must NOT have been stripped,
    // meaning that it must still have its symbol table section. Otherwise, relocations will not work.  
    if find_first_section_by_type(&elf_file, ShType::SymTab).is_none() {
        error!("parse_elf_kernel_crate(): couldn't find symbol table -- is file \"{}\" stripped?", module_name);
        return Err("no symbol table found -- cannot load!");
    }

    // Calculate how many bytes (and thus how many pages) we need for each of the three section types,
    // which are text (present | exec), rodata (present | noexec), data (present | writable)
    let (text_bytecount, rodata_bytecount, data_bytecount): (usize, usize, usize) = {
        let (mut text, mut rodata, mut data) = (0, 0, 0);
        for sec in elf_file.section_iter() {
            if let Ok(ShType::ProgBits) = sec.get_type() {
                let size = sec.size() as usize;
                if (size == 0) || (sec.flags() & SHF_ALLOC == 0) {
                    continue; // skip non-allocated PROGBITS sections (they're useless)
                }
                // filter flags for ones we care about (we already checked that it's loaded (SHF_ALLOC))
                let write = sec.flags() & SHF_WRITE == SHF_WRITE;
                let exec  = sec.flags() & SHF_EXECINSTR == SHF_EXECINSTR;
                if exec {
                    text += size;
                }
                else if write {
                    data += size;
                }
                else {
                    rodata += size;
                }
            }
        }
        (text, rodata, data)
    };

    // create a closure here to allocate N contiguous virtual memory pages
    // and map them to random frames as writable, returns Result<OwnedContiguousPages, &'static str>
    let (text_pages, rodata_pages, data_pages): (Result<OwnedContiguousPages, &'static str>,
                                                 Result<OwnedContiguousPages, &'static str>, 
                                                 Result<OwnedContiguousPages, &'static str>) = {
        let mut allocate_pages_closure = |size_in_bytes: usize| {
            use memory::virtual_address_allocator::allocate_pages_by_bytes;
            let allocated_pages = try!(allocate_pages_by_bytes(size_in_bytes));
            use memory::FRAME_ALLOCATOR;
            let mut frame_allocator = FRAME_ALLOCATOR.try().unwrap().lock();

            // right now we're just simply copying small sections to the new memory
            // so we have to map those pages to real (randomly chosen) frames first
            for p in 0..allocated_pages.num_pages {
                // because we're copying bytes to the newly allocated pages, we need to make them writaable too
                active_table.map(allocated_pages.start + p, EntryFlags::PRESENT | EntryFlags::WRITABLE, frame_allocator.deref_mut());
            }
            Ok(allocated_pages)
        };

        // we must allocated these pages separately because they will have different flags
        (
            allocate_pages_closure(text_bytecount), 
            allocate_pages_closure(rodata_bytecount), 
            allocate_pages_closure(data_bytecount)
        )
    };


    // First, we need to parse all the sections and load the text and data sections
    let mut sections: Vec<LoadedSection> = Vec::new();
    let mut text_offset: Option<usize> = None;
    let mut rodata_offset: Option<usize> = None;
    let mut data_offset: Option<usize> = None;
    {
        for sec in elf_file.section_iter() {
            // the PROGBITS sections are the bulk of what we care about, i.e., .text & data sections
            if let Ok(ShType::ProgBits) = sec.get_type() {
                // skip null section and any empty sections
                let sec_size = sec.size() as usize;
                if sec_size == 0 { continue; }

                const TEXT_PREFIX:   &'static str = ".text.";
                const RODATA_PREFIX: &'static str = ".rodata.";
                const DATA_PREFIX:   &'static str = ".data.";
                let text_prefix_end:   usize = TEXT_PREFIX.len();
                let rodata_prefix_len: usize = RODATA_PREFIX.len();
                let data_prefix_len:   usize = DATA_PREFIX.len();

                if let Ok(name) = sec.get_name(&elf_file) {

                    if name.starts_with(TEXT_PREFIX) {
                        if let Some(name) = name.get(text_prefix_end..) {
                            let demangled: String = demangle(name).to_string();
                            trace!("Found .text section: {:?} {:?}, size={:#x}",
                                    name, demangled, sec_size);
                            assert!(sec.flags() & (SHF_ALLOC | SHF_EXECINSTR) == (SHF_ALLOC | SHF_EXECINSTR), ".text section had wrong flags!");
                            // let entry_flags = EntryFlags::from_elf_section_flags(sec.flags());

                            if text_offset.is_none() {
                                text_offset = Some(sec.offset() as usize);
                            }

                            if let Ok(ref tp) = text_pages {
                                let dest_addr = tp.start.start_address() + (sec.offset() as usize) - text_offset.unwrap();

                                // here: we're ready to copy the data/text section to the proper address
                                if let Ok(SectionData::Undefined(sec_data)) = sec.get_data(&elf_file) {
                                    // SAFE: we have allocated the pages containing section_vaddr and mapped them above
                                    let dest: &mut [u8] = unsafe {
                                        slice::from_raw_parts_mut(dest_addr as *mut u8, sec_size) 
                                    };
                                    dest.copy_from_slice(sec_data);

                                    sections.push( LoadedSection::Text(
                                        TextSection{
                                            symbol: demangled.clone(), // FIXME
                                            abs_symbol: demangled.clone(), // FIXME
                                            hash: None, // FIXME
                                            virt_addr: dest_addr,
                                            size: sec_size,
                                        }
                                    ));
                                }
                                else {
                                    error!("expected \"Undefined\" data in .text section {}: {:?}", name, sec.get_data(&elf_file));
                                    return Err("unexpected data in .text section");
                                }
                            }
                            else {
                                error!("trying to load text section, but no text_pages were allocated!!");
                                return Err("no text_pages were allocated");
                            }
                        }
                    }
                    else if name.starts_with(RODATA_PREFIX) {
                        warn!("skipping unhandled RODATA section {:?}", sec);
                        continue; // TODO handle this
                    }

                    else if name.starts_with(DATA_PREFIX) {
                        warn!("skipping unhandled DATA section {:?}", sec);
                        continue; // TODO handle this
                    }

                    else {
                        warn!("skipping unhandled PROGBITS section {:?}", sec);
                        continue;
                    }

                }
                else {
                    warn!("parse_elf_kernel_crate: couldn't get section name for {:?}", sec);
                    return Err("couldn't get section name");
                }
            }
        }
    } // end of handling PROGBITS sections: text, data, rodata


    // Second, we need to fix up the sections we just loaded with proper relocation info
    for sec in elf_file.section_iter() {
        if let Ok(ShType::Rela) = sec.get_type() {
            // skip null section and any empty sections
            let sec_size = sec.size() as usize;
            if sec_size == 0 { continue; }

            // offset is the destination 
            use xmas_elf::sections::SectionData::Rela64;
            trace!("Found Rela section: {:?}", sec);
            if let Ok(Rela64(rela_arr)) = sec.get_data(&elf_file) {
                trace!("      Rela64 data: {:?}", rela_arr);
                for r in rela_arr {
                    trace!("      offset: {:#X}, addend: {:#X}, symtab_index: {:#X}, type: {:#X}",
                                r.get_offset(), r.get_addend(), r.get_symbol_table_index(), r.get_type());
                }
                    

            }
        }
    }

    
    // since we initially mapped the pages as writable, we need to remap them properly according to each section
    let all_pages = {
        let mut all_pages: Vec<OwnedContiguousPages> = Vec::new();
        
        let mut remap = |allocated_pages: &OwnedContiguousPages, flags| {
            for p in 0..allocated_pages.num_pages {
                active_table.remap(allocated_pages.start + p, flags);
            }
        };

        if let Ok(tp) = text_pages { 
            remap(&tp, EntryFlags::PRESENT); // present and not noexec
            all_pages.push(tp); 
        }
        if let Ok(rp) = rodata_pages { 
            remap(&rp, EntryFlags::PRESENT | EntryFlags::NO_EXECUTE); // present (just readable)
            all_pages.push(rp); 
        }
        if let Ok(dp) = data_pages { 
            remap(&dp, EntryFlags::PRESENT | EntryFlags::WRITABLE | EntryFlags::NO_EXECUTE); // read/write
            all_pages.push(dp); 
        }

        all_pages
    };
    

    Ok(LoadedCrate {
        crate_name: String::from(module_name.get(3..).unwrap()),
        sections: sections,
        owned_pages: all_pages,
    })

}




/// Finds a section of the given `ShType` and returns the "first" one 
/// based on the (potentially random) ordering of sections in the given `ElfFile`.
pub fn find_first_section_by_type<'a>(elf_file: &'a ElfFile, typ: ShType) -> Option<SectionHeader<'a>> {
    for sec in elf_file.section_iter() {
        if let Ok(sec_type) = sec.get_type() {
            if typ == sec_type {
                return Some(sec);
            }
        }
    }

    None
}