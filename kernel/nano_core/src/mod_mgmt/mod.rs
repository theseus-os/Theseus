use xmas_elf;
use xmas_elf::ElfFile;
use xmas_elf::sections::{SectionHeader, SectionData, ShType};
use xmas_elf::sections::{SHF_WRITE, SHF_ALLOC, SHF_EXECINSTR};
use core::slice;
use core::ops::DerefMut;
use alloc::{Vec, BTreeMap, BTreeSet, String};
use alloc::arc::Arc;
use alloc::string::ToString;
use memory::{VirtualMemoryArea, VirtualAddress, MappedPages, EntryFlags, ActivePageTable, allocate_pages_by_bytes};
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

    // check that elf_file is an executable type 
    {
        use xmas_elf::header::Type;
        let typ = elf_file.header.pt2.type_().as_type();
        if typ != Type::Executable {
            error!("parse_elf_executable(): ELF file has wrong type {:?}, must be an Executable Elf File!", typ);
            return Err("not a relocatable elf file");
        }
    } 

    let mut prog_sects: Vec<ElfProgramSegment> = Vec::new();
    for prog in elf_file.program_iter() {
        // debug!("   prog: {}", prog);
        use xmas_elf::program::Type;
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



pub fn parse_elf_kernel_crate(mapped_pages: MappedPages, size: usize, module_name: &String, active_table: &mut ActivePageTable)
    -> Result<LoadedCrate, &'static str>
{
    // all kernel module crate names must start with "__k_"
    const KERNEL_MODULE_NAME_PREFIX: &'static str = "__k_";

    let start_addr = mapped_pages.start_address() as usize as *const u8;
    debug!("Parsing Elf kernel crate: {:?}, start_addr {:#x}, size {:#x}({})", module_name, start_addr as usize, size, size);
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

    // check that elf_file is a relocatable type 
    {
        use xmas_elf::header::Type;
        let typ = elf_file.header.pt2.type_().as_type();
        if typ != Type::Relocatable {
            error!("parse_elf_kernel_crate(): module {} was of type {:?}, must be a Relocatable Elf File!", module_name, typ);
            return Err("not a relocatable elf file");
        }
    } 


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

    // iterate through the symbol table so we can find which sections are global (publicly visible)
    // we keep track of them here in a list
    let global_sections: BTreeSet<usize> = {
        let mut globals: BTreeSet<usize> = BTreeSet::new();
        use xmas_elf::symbol_table::Entry;
        for entry in symtab.iter() {
            if let Ok(typ) = entry.get_type() {
                if typ == xmas_elf::symbol_table::Type::Func || typ == xmas_elf::symbol_table::Type::Object {
                    use xmas_elf::symbol_table::Visibility;
                    match entry.get_other() {
                        Visibility::Default => {
                            if let Ok(bind) = entry.get_binding() {
                                if bind == xmas_elf::symbol_table::Binding::Global {
                                    globals.insert(entry.shndx() as usize);
                                }
                            }
                        }
                        _ => {
                            continue;
                        }
                    }
                }
            }
        }   
        globals 
    };

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
    // and map them to random frames as writable, returns Result<MappedPages, &'static str>
    let (text_pages, rodata_pages, data_pages): (Result<MappedPages, &'static str>,
                                                 Result<MappedPages, &'static str>, 
                                                 Result<MappedPages, &'static str>) = {
        use memory::FRAME_ALLOCATOR;
        let mut frame_allocator = try!(FRAME_ALLOCATOR.try().ok_or("couldn't get FRAME_ALLOCATOR")).lock();

        let mut allocate_pages_closure = |size_in_bytes: usize| {
            let allocated_pages = try!(allocate_pages_by_bytes(size_in_bytes).ok_or("Couldn't allocate_pages_by_bytes, out of virtual address space"));

            // Right now we're just simply copying small sections to the new memory,
            // so we have to map those pages to real (randomly chosen) frames first. 
            // because we're copying bytes to the newly allocated pages, we need to make them writeable too, 
            // and then change the page permissions (by using remap) later. 
            active_table.map_allocated_pages(allocated_pages, EntryFlags::PRESENT | EntryFlags::WRITABLE, frame_allocator.deref_mut())
        };

        // we must allocate these pages separately because they will have different flags
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
                                let dest_addr = tp.start_address() + (sec.offset() as usize) - text_offset.unwrap();

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
                                            global: global_sections.contains(&shndx),
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
                        let rodata_prefix_end:   usize = RODATA_PREFIX.len();
                        if let Some(name) = name.get(rodata_prefix_end..) {
                            let demangled = demangle_symbol(name);
                            trace!("Found .rodata section: name {:?}, with_hash {:?}, size={:#x}", name, demangled.full, sec_size);
                            assert!(sec.flags() & (SHF_ALLOC | SHF_WRITE | SHF_EXECINSTR) == (SHF_ALLOC), ".rodata section had wrong flags!");
                            
                            if rodata_offset.is_none() {
                                rodata_offset = Some(sec.offset() as usize);
                            }

                            if let Ok(ref rp) = rodata_pages {
                                let dest_addr = rp.start_address() + (sec.offset() as usize) - rodata_offset.unwrap();

                                // here: we're ready to copy the data/text section to the proper address
                                if let Ok(SectionData::Undefined(sec_data)) = sec.get_data(&elf_file) {
                                    // SAFE: we have allocated the pages containing section_vaddr and mapped them above
                                    let dest: &mut [u8] = unsafe {
                                        slice::from_raw_parts_mut(dest_addr as *mut u8, sec_size) 
                                    };
                                    dest.copy_from_slice(sec_data);

                                    loaded_sections.insert(shndx, 
                                        Arc::new( LoadedSection::Rodata(RodataSection{
                                            symbol: demangled.symbol,
                                            abs_symbol: demangled.full,
                                            hash: demangled.hash,
                                            virt_addr: dest_addr,
                                            size: sec_size,
                                            global: global_sections.contains(&shndx),
                                        }))
                                    );
                                }
                                else {
                                    error!("expected \"Undefined\" data in .rodata section {}: {:?}", name, sec.get_data(&elf_file));
                                    return Err("unexpected data in .rodata section");
                                }
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
                                let dest_addr = dp.start_address() + (sec.offset() as usize) - data_offset.unwrap();

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
                                            global: global_sections.contains(&shndx),
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
            use xmas_elf::sections::SectionData::Rela64;
            use xmas_elf::symbol_table::Entry;
            trace!("Found Rela section name: {:?}, type: {:?}, target_sec_index: {:?}", sec.get_name(&elf_file), sec.get_type(), sec.info());

            // currently not using eh_frame sections
            if let Ok(name) = sec.get_name(&elf_file) {
                if name.contains("eh_frame") {
                    continue;
                }
            }

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
                                unimplemented!(); // if we stop using the large code model, we need to create a Global Offset Table
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
    let mut all_pages: Vec<MappedPages> = Vec::with_capacity(3); // max 3, for text, rodata, data
    if let Ok(tp) = text_pages { 
        try!(active_table.remap(&tp, EntryFlags::PRESENT)); // present and not noexec
        all_pages.push(tp);
    }
    if let Ok(rp) = rodata_pages { 
        try!(active_table.remap(&rp, EntryFlags::PRESENT | EntryFlags::NO_EXECUTE)); // present (just readable)
        all_pages.push(rp);
    }
    if let Ok(dp) = data_pages { 
        try!(active_table.remap(&dp, EntryFlags::PRESENT | EntryFlags::WRITABLE | EntryFlags::NO_EXECUTE)); // read/write
        all_pages.push(dp);
    }
    
    // extract just the sections from the section map
    let (_keys, values): (Vec<usize>, Vec<Arc<LoadedSection>>) = loaded_sections.into_iter().unzip();
    let kernel_module_name_prefix_end = KERNEL_MODULE_NAME_PREFIX.len();


    Ok(LoadedCrate {
        crate_name: String::from(module_name.get(kernel_module_name_prefix_end..).unwrap()), 
        sections: values,
        mapped_pages: all_pages,
    })

}



// parses the nano_core ELF file, which is not loaded (because it is already loaded and running right out of the gate) 
// but rather searched for global symbols, which are added to the system map and the crate metadata
pub fn parse_nano_core(mapped_pages: MappedPages, size: usize) -> Result<LoadedCrate, &'static str> {
    let start_addr = mapped_pages.start_address() as usize as *const u8;
    debug!("Parsing nano_core: start_addr {:#x}, size {:#x}({})", start_addr as usize, size, size);
    if start_addr.is_null() {
        error!("parse_nano_core(): start_addr is null!");
        return Err("start_addr for parse_nano_core is null!");
    }

    // SAFE: checked for null
    let byte_slice = unsafe { slice::from_raw_parts(start_addr, size) };
    // debug!("BYTE SLICE: {:?}", byte_slice);
    let elf_file = try!(ElfFile::new(byte_slice)); // returns Err(&str) if ELF parse fails


    // For us to properly load the ELF file, it must NOT have been stripped,
    // meaning that it must still have its symbol table section. Otherwise, relocations will not work.
    use xmas_elf::sections::SectionData::SymbolTable64;
    let sssec = find_first_section_by_type(&elf_file, ShType::SymTab);
    let symtab_data = match sssec.ok_or("no symtab section").and_then(|s| s.get_data(&elf_file)) {
        Ok(SymbolTable64(symtab)) => Ok(symtab),
        _ => {
            error!("parse_nano_core(): can't load file: no symbol table found. Was file stripped?");
            Err("cannot load nano_core: no symbol table found. Was file stripped?")
        }
    };
    let symtab = try!(symtab_data);
    // debug!("symtab: {:?}", symtab);

    
    // find the .text, .data, and .rodata sections
    let mut text_shndx:   Option<usize> = None;
    let mut rodata_shndx: Option<usize> = None;
    let mut data_shndx:   Option<usize> = None;
    let mut bss_shndx:    Option<usize> = None;

    for (shndx, sec) in elf_file.section_iter().enumerate() {
        // the PROGBITS sections are the bulk of what we care about, i.e., .text & data sections
        if let Ok(ShType::ProgBits) = sec.get_type() {
            // skip null section and any empty sections
            let sec_size = sec.size() as usize;
            if sec_size == 0 { continue; }

            if let Ok(name) = sec.get_name(&elf_file) {
                match name {
                    ".text" => {
                        assert!(sec.flags() & (SHF_ALLOC | SHF_WRITE | SHF_EXECINSTR) == (SHF_ALLOC | SHF_EXECINSTR), ".text section had wrong flags!");
                        text_shndx = Some(shndx);
                    }
                    ".data" => {
                        assert!(sec.flags() & (SHF_ALLOC | SHF_WRITE | SHF_EXECINSTR) == (SHF_ALLOC | SHF_WRITE), ".data section had wrong flags!");
                        data_shndx = Some(shndx);
                    }
                    ".rodata" => {
                        assert!(sec.flags() & (SHF_ALLOC | SHF_WRITE | SHF_EXECINSTR) == (SHF_ALLOC), ".rodata section had wrong flags!");
                        rodata_shndx = Some(shndx);
                    }
                    _ => {
                        continue;
                    }
                };
            }
        }
        // look for .bss section
        else if let Ok(ShType::NoBits) = sec.get_type() {
            // skip null section and any empty sections
            let sec_size = sec.size() as usize;
            if sec_size == 0 { continue; }

            if let Ok(name) = sec.get_name(&elf_file) {
                if name == ".bss" {
                    assert!(sec.flags() & (SHF_ALLOC | SHF_WRITE | SHF_EXECINSTR) == (SHF_ALLOC | SHF_WRITE), ".bss section had wrong flags!");
                    bss_shndx = Some(shndx);
                }
            }
        }
    }

    let text_shndx   = try!(text_shndx.ok_or("couldn't find .text section in nano_core ELF"));
    let rodata_shndx = try!(rodata_shndx.ok_or("couldn't find .rodata section in nano_core ELF"));
    let data_shndx   = try!(data_shndx.ok_or("couldn't find .data section in nano_core ELF"));
    let bss_shndx    = try!(bss_shndx.ok_or("couldn't find .bss section in nano_core ELF"));

    // iterate through the symbol table so we can find which sections are global (publicly visible)

    let loaded_sections = {
        let mut sections: Vec<Arc<LoadedSection>> = Vec::new();
        use xmas_elf::symbol_table::Entry;
        for entry in symtab.iter() {
            use xmas_elf::symbol_table::Visibility;
            match entry.get_other() {
                Visibility::Default => {
                    // do nothing, fall through to proceed
                }
                _ => {
                    continue; // skip this
                }
            };
            
            if let Ok(typ) = entry.get_type() {
                if typ == xmas_elf::symbol_table::Type::Func || typ == xmas_elf::symbol_table::Type::Object {
                    if let Ok(bind) = entry.get_binding() {
                        if bind == xmas_elf::symbol_table::Binding::Global {
                            let sec_vaddr = entry.value() as VirtualAddress;
                            let name = match entry.get_name(&elf_file) {
                                Ok(n) => n,
                                _ => {
                                    warn!("parse_nano_core(): couldn't get_name(), skipping entry {:?}", entry);
                                    continue;
                                }
                            };
                            // debug!("parse_nano_core(): name: {}, vaddr: {:#X}", name, sec_vaddr);

                            let demangled = demangle_symbol(name);
                            let new_section = {
                                if entry.shndx() as usize == text_shndx {
                                    Some(LoadedSection::Text(TextSection{
                                        symbol: demangled.symbol,
                                        abs_symbol: demangled.full,
                                        hash: demangled.hash,
                                        virt_addr: sec_vaddr,
                                        size: 0, // TODO FIXME: is it necessary to calculate the size?
                                        global: true,
                                    }))
                                }
                                else if entry.shndx() as usize == rodata_shndx {
                                    Some(LoadedSection::Rodata(RodataSection{
                                        symbol: demangled.symbol,
                                        abs_symbol: demangled.full,
                                        hash: demangled.hash,
                                        virt_addr: sec_vaddr,
                                        size: 0, // TODO FIXME: is it necessary to calculate the size?
                                        global: true,
                                    }))
                                }
                                else if (entry.shndx() as usize == data_shndx) || (entry.shndx() as usize == bss_shndx) {
                                    Some(LoadedSection::Data(DataSection{
                                        symbol: demangled.symbol,
                                        abs_symbol: demangled.full,
                                        hash: demangled.hash,
                                        virt_addr: sec_vaddr,
                                        size: 0, // TODO FIXME: is it necessary to calculate the size?
                                        global: true,
                                    }))
                                }
                                else {
                                    error!("Unexpected entry.shndx(): {}", entry.shndx());
                                    None
                                }
                            };

                            if new_section.is_some() {
                                sections.push(Arc::new(new_section.unwrap()));
                            }
                        }
                    }
                }
            }
        }   
        sections 
    };


    Ok(LoadedCrate {
        crate_name: String::from("nano_core"), 
        sections: loaded_sections,
        mapped_pages: vec![mapped_pages],
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