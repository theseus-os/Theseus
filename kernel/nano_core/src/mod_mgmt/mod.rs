use xmas_elf::ElfFile;
use xmas_elf::program::Type;
use xmas_elf::sections::{SectionHeader, SectionData, ShType};
use xmas_elf::sections::{SHF_WRITE, SHF_ALLOC, SHF_EXECINSTR};
use core::mem;
use core::slice;
use core::ptr;
use core::ops::DerefMut;
use spin::Mutex;
use alloc::{Vec, BTreeMap, String};
use alloc::arc::{Arc, Weak};
use alloc::string::ToString;
use memory::{VirtualMemoryArea, VirtualAddress, PhysicalAddress, EntryFlags, ActivePageTable, FRAME_ALLOCATOR};
use memory::virtual_address_allocator::OwnedContiguousPages;
use kernel_config::memory::{PAGE_SIZE, BYTES_PER_ADDR};
use goblin::elf::reloc::*;


pub mod metadata;
use self::metadata::*;

// Can also try this crate: https://crates.io/crates/goblin
// ELF RESOURCE: http://www.cirosantilli.com/elf-hello-world


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

    // TODO FIXME: check elf_file is an executable type 

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

/// A representation of a demangled symbol, e.g., my_crate::module::func_name.
/// If the symbol wasn't originally mangled, `symbol` == `full`. 
struct DemangledSymbol {
    symbol: String,
    full: String, 
    hash: Option<String>,
}

fn demangle_symbol(s: &str) -> DemangledSymbol {
    use rustc_demangle::demangle;
    let demangled = demangle(s);
    let without_hash: String = format!("{:#}", demangled); // the fully-qualified symbol, no hash
    let symbol_only: Option<String> = without_hash.rsplit("::").next().map(|s| s.to_string()); // last thing after "::", excluding the hash
    let with_hash: String = format!("{}", demangled); // the fully-qualified symbol, with the hash
    let hash_only: Option<String> = with_hash.find::<&str>(without_hash.as_ref())
        .and_then(|index| {
            let hash_start = index + 2 + without_hash.len();
            with_hash.get(hash_start..).map(|s| s.to_string())
        }); // + 2 to skip the "::" separator
    
    DemangledSymbol {
        symbol: symbol_only.unwrap_or(without_hash.clone()),
        full: without_hash,
        hash: hash_only,
    }
}



pub fn parse_elf_kernel_crate(start_addr: VirtualAddress, size: usize, module_name: &str, active_table: &mut ActivePageTable)
    -> Result<LoadedCrate, &'static str>
{
    // all kernel module crate names must start with "__k_"
    const KERNEL_MODULE_NAME_PREFIX: &'static str = "__k_";

    debug!("Parsing Elf kernel crate: {:?}, start_addr {:#x}, size {:#x}({})", module_name, start_addr as usize, size, size);
    let start_addr = start_addr as *const u8;
    if start_addr.is_null() {
        error!("parse_elf_kernel_crate(): start_addr is null!");
        return Err("start_addr for parse_elf_kernel_crate is null!");
    }
    if !module_name.starts_with("__k_") {
        error!("parse_elf_kernel_crate(): error parsing crate: {}, name must start with {}.", module_name, KERNEL_MODULE_NAME_PREFIX);
        return Err("module_name didn't start with __k_");
    }

    // SAFE: checked for null
    let byte_slice = unsafe { slice::from_raw_parts(start_addr, size) };
    // debug!("BYTE SLICE: {:?}", byte_slice);
    let elf_file = try!(ElfFile::new(byte_slice)); // returns Err(&str) if ELF parse fails

    // TODO FIXME: check elf_file is a relocatable type 

    // For us to properly load the ELF file, it must NOT have been stripped,
    // meaning that it must still have its symbol table section. Otherwise, relocations will not work.
    use xmas_elf::sections::SectionData::SymbolTable64;
    let symtab_data = match find_first_section_by_type(&elf_file, ShType::SymTab).ok_or("no symtab section").and_then(|s| s.get_data(&elf_file)) {
        Ok(SymbolTable64(symtab)) => Ok(symtab),
        _ => {
            error!("parse_elf_kernel_crate(): can't load file: no symbol table found. Was file stripped?");
            Err("cannot load: no symbol table found. Was file stripped?")
        }
    };
    let symtab = try!(symtab_data);
    // debug!("symtab: {:?}", symtab);

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
    let mut loaded_sections: BTreeMap<usize, Arc<LoadedSection>> = BTreeMap::new(); // map section header index (shndx) to LoadedSection
    let mut text_offset: Option<usize> = None;
    let mut rodata_offset: Option<usize> = None;
    let mut data_offset: Option<usize> = None;
    {
        for (shndx, sec) in elf_file.section_iter().enumerate() {
            // the PROGBITS sections are the bulk of what we care about, i.e., .text & data sections
            if let Ok(ShType::ProgBits) = sec.get_type() {
                // skip null section and any empty sections
                let sec_size = sec.size() as usize;
                if sec_size == 0 { continue; }


                if let Ok(name) = sec.get_name(&elf_file) {
               
                    const TEXT_PREFIX:   &'static str = ".text.";
                    const RODATA_PREFIX: &'static str = ".rodata.";
                    const DATA_PREFIX:   &'static str = ".data.";

                    if name.starts_with(TEXT_PREFIX) {
                        let text_prefix_end:   usize = TEXT_PREFIX.len();
                        if let Some(name) = name.get(text_prefix_end..) {
                            let demangled = demangle_symbol(name);
                            trace!("Found .text section: name {:?}, with_hash {:?}, size={:#x}", name, demangled.full, sec_size);
                            assert!(sec.flags() & (SHF_ALLOC | SHF_WRITE | SHF_EXECINSTR) == (SHF_ALLOC | SHF_EXECINSTR), ".text section had wrong flags!");
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

                                    loaded_sections.insert(shndx, 
                                        Arc::new( LoadedSection::Text(TextSection{
                                            symbol: demangled.symbol,
                                            abs_symbol: demangled.full,
                                            hash: demangled.hash,
                                            virt_addr: dest_addr,
                                            size: sec_size,
                                        }))
                                    );
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
                        else {
                            error!("Failed to get the .text section's name after \".text.\": {:?}", name);
                            return Err("Failed to get the .text section's name after \".text.\"!");
                        }
                    }

                    else if name.starts_with(RODATA_PREFIX) {
                        trace!("Found .rodata section: {:?}", name);
                        assert!(sec.flags() & (SHF_ALLOC | SHF_WRITE | SHF_EXECINSTR) == (SHF_ALLOC), ".rodata section had wrong flags!");
                        
                        if rodata_offset.is_none() {
                            rodata_offset = Some(sec.offset() as usize);
                        }

                        // let rodata_prefix_len: usize = RODATA_PREFIX.len();

                        if let Ok(ref rp) = rodata_pages {
                            let dest_addr = rp.start.start_address() + (sec.offset() as usize) - rodata_offset.unwrap();

                            // here: we're ready to copy the data/text section to the proper address
                            if let Ok(SectionData::Undefined(sec_data)) = sec.get_data(&elf_file) {
                                // SAFE: we have allocated the pages containing section_vaddr and mapped them above
                                let dest: &mut [u8] = unsafe {
                                    slice::from_raw_parts_mut(dest_addr as *mut u8, sec_size) 
                                };
                                dest.copy_from_slice(sec_data);

                                loaded_sections.insert(shndx, 
                                    Arc::new( LoadedSection::Rodata(RodataSection{
                                        virt_addr: dest_addr,
                                        size: sec_size,
                                    }))
                                );
                            }
                            else {
                                error!("expected \"Undefined\" data in .rodata section {}: {:?}", name, sec.get_data(&elf_file));
                                return Err("unexpected data in .rodata section");
                            }
                        }
                    }

                    else if name.starts_with(DATA_PREFIX) {
                        let data_prefix_len:   usize = DATA_PREFIX.len();
                        if let Some(name) = name.get(data_prefix_len..) {
                            let demangled = demangle_symbol(name);
                            trace!("Found .data section: name {:?}, with_hash {:?}, size={:#x}", name, demangled.full, sec_size);
                            assert!(sec.flags() & (SHF_ALLOC | SHF_WRITE | SHF_EXECINSTR) == (SHF_ALLOC | SHF_WRITE), ".data section had wrong flags!");
                            
                            if data_offset.is_none() {
                                data_offset = Some(sec.offset() as usize);
                            }

                            if let Ok(ref dp) = data_pages {
                                let dest_addr = dp.start.start_address() + (sec.offset() as usize) - data_offset.unwrap();

                                // here: we're ready to copy the data/text section to the proper address
                                if let Ok(SectionData::Undefined(sec_data)) = sec.get_data(&elf_file) {
                                    // SAFE: we have allocated the pages containing section_vaddr and mapped them above
                                    let dest: &mut [u8] = unsafe {
                                        slice::from_raw_parts_mut(dest_addr as *mut u8, sec_size) 
                                    };
                                    dest.copy_from_slice(sec_data);

                                    loaded_sections.insert(shndx, 
                                        Arc::new( LoadedSection::Data(DataSection{
                                            symbol: demangled.symbol,
                                            abs_symbol: demangled.full,
                                            hash: demangled.hash,
                                            virt_addr: dest_addr,
                                            size: sec_size,
                                        }))
                                    );
                                }
                                else {
                                    error!("expected \"Undefined\" data in .data section {}: {:?}", name, sec.get_data(&elf_file));
                                    return Err("unexpected data in .data section");
                                }
                            }
                        }
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
            use xmas_elf::sections::SectionData::{Rela32, Rela64};
            use xmas_elf::symbol_table::Entry;
            trace!("Found Rela section name: {:?}, type: {:?}, target_sec_index: {:?}", sec.get_name(&elf_file), sec.get_type(), sec.info());

            // the target section is where we write the relocation data to.
            // the source section is where we get the data from. 
            // There is one target section per rela section, and one source section per entry in this rela section.
            // The "info" field in the Rela section specifies which section is the target of the relocation.
            
            // check if this Rela sections has a valid target section (one that we've already loaded)
            if let Some(target_sec) = loaded_sections.get(&(sec.info() as usize)) {
                if let Ok(Rela64(rela_arr)) = sec.get_data(&elf_file) {
                    for r in rela_arr {
                        trace!("      Rela64 offset: {:#X}, addend: {:#X}, symtab_index: {:#X}, type: {:#X}",
                                r.get_offset(), r.get_addend(), r.get_symbol_table_index(), r.get_type());

                        // common to all relocations: calculate the relocation destination and get the source section
                        let dest_offset = r.get_offset() as usize;
                        let dest_ptr: usize = target_sec.virt_addr() + dest_offset;
                        let source_sec_entry: &Entry = &symtab[r.get_symbol_table_index() as usize];
                        let source_sec_shndx: usize = source_sec_entry.shndx() as usize; 
                        let source_sec_name = try!(source_sec_entry.get_name(&elf_file));
                        trace!("             relevant section {:?} -- {:?}", source_sec_name, source_sec_entry.get_section_header(&elf_file, r.get_symbol_table_index() as usize).and_then(|s| s.get_name(&elf_file)));
                        let source_sec: Arc<LoadedSection> = try!(
                            loaded_sections.get(&source_sec_shndx).cloned().or_else(|| { 
                                // the source section was not in this object file, so check our list of loaded external crates 
                                // do a quick search for the symbol's demangled name in the kernel's symbol map
                                let demangled = demangle_symbol(source_sec_name);
                                metadata::get_symbol(demangled.full).upgrade()
                            })
                            .ok_or_else(|| {
                                error!("Could not resolve source section for symbol relocation for symtab[{}] {:?}", source_sec_shndx, source_sec_name);
                                "Could not resolve source section for symbol relocation"
                            })
                        );
                        
                        

                        // There is a great, succint table of relocation types here
                        // https://docs.rs/goblin/0.0.13/goblin/elf/reloc/index.html
                        match r.get_type() {
                            R_X86_64_32 => {
                                let source_val = source_sec.virt_addr().wrapping_add(r.get_addend() as usize);
                                trace!("                    dest_ptr: {:#X}, source_val: {:#X} ({:?})", dest_ptr, source_val, source_sec);
                                unsafe {
                                    *(dest_ptr as *mut u32) = source_val as u32;
                                }
                            }
                            R_X86_64_64 => {
                                let source_val = source_sec.virt_addr().wrapping_add(r.get_addend() as usize);
                                trace!("                    dest_ptr: {:#X}, source_val: {:#X} ({:?})", dest_ptr, source_val, source_sec);
                                unsafe {
                                    *(dest_ptr as *mut u64) = source_val as u64;
                                }
                            }
                            R_X86_64_PC32 => {
                                let source_val = source_sec.virt_addr().wrapping_add(r.get_addend() as usize) - dest_ptr;
                                trace!("                    dest_ptr: {:#X}, source_val: {:#X} ({:?})", dest_ptr, source_val, source_sec);
                                unsafe {
                                    *(dest_ptr as *mut u32) = source_val as u32;
                                }
                            }
                            R_X86_64_PC64 => {
                                let source_val = source_sec.virt_addr().wrapping_add(r.get_addend() as usize) - dest_ptr;
                                trace!("                    dest_ptr: {:#X}, source_val: {:#X} ({:?})", dest_ptr, source_val, source_sec);
                                unsafe {
                                    *(dest_ptr as *mut u64) = source_val as u64;
                                }
                            }
                            R_X86_64_GOTPCREL => { 
                                unimplemented!(); // TODO FIXME we need to create a Global Offset Table
                            }
                            _ => {
                                error!("found unsupported relocation {:?}\n  --> Are you building kernel modules with code-model=large?", r);
                                return Err("found unsupported relocation type");
                            }
                        }   
                    }
                }
                else {
                    error!("Found Rela section that wasn't able to be parsed as Rela64: {:?}", sec);
                    return Err("Found Rela section that wasn't able to be parsed as Rela64");
                }
            }
            else {
                warn!("Skipping Rela section {:?} for target section that wasn't loaded!", sec.get_name(&elf_file));
                continue;
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
    
    // extract just the sections from the section map
    let (_keys, values): (Vec<usize>, Vec<Arc<LoadedSection>>) = loaded_sections.into_iter().unzip();
    let kernel_module_name_prefix_end = KERNEL_MODULE_NAME_PREFIX.len();

    Ok(LoadedCrate {
        crate_name: String::from(module_name.get(kernel_module_name_prefix_end..).unwrap()), 
        sections: values,
        owned_pages: all_pages,
    })

}


// // check that we  have a valid target section, for now we only care about applying relocations to actual loaded sections, i.e., PROGBITS
            // {
            //     let target_sec_symtab_entry: &Entry = &symtab[sec.info() as usize];
            //     let target_sec_hdr = target_sec_symtab_entry.get_section_header(&elf_file, sec.info() as usize); 
            //     if let Ok(tsh) = target_sec_hdr {
            //         if tsh.get_type() != ShType::ProgBits {
            //             info!("Skipping relocation for non-PROGBITS section {:?}", target_sec_hdr.and_then(|s| s.get_name(&elf_file))))
            //             continue;
            //         }
            //     }
            //     else {
            //         warn!("Rela section specified a target section index {} that didn't correspond to a real section!", sec.info());
            //         continue;
            //     }
            // }





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
