//! Routines for parsing the `nano_core`, the fully-linked, already loaded code that is currently executing.
//! As such, it performs no loading, but rather just creates metadata that represents
//! the existing kernel code that was loaded by the bootloader, and adds those functions to the system map.


use core::ops::DerefMut;
use alloc::{Vec, String};
use alloc::arc::Arc;
use alloc::string::ToString;
use spin::{Mutex, RwLock};

use xmas_elf;
use xmas_elf::ElfFile;
use xmas_elf::sections::ShType;
use xmas_elf::sections::{SHF_WRITE, SHF_ALLOC, SHF_EXECINSTR};

use memory::{FRAME_ALLOCATOR, get_module, MemoryManagementInfo, Frame, PageTable, VirtualAddress, MappedPages, EntryFlags, allocate_pages_by_bytes};

use metadata::{LoadedSection, StrongSectionRef, LoadedCrate, SectionType, add_crate};


/// Decides which parsing technique to use, either the symbol file or the actual binary file.
/// `parse_nano_core_binary()` is VERY SLOW in debug mode for large binaries, so we use the more efficient symbol file parser instead.
/// Note that this must match the setup of the kernel/Makefile as well as the cfg/grub.cfg entry
const PARSE_NANO_CORE_SYMBOL_FILE: bool = true;


// Parses the nano_core module that represents the already loaded (and currently running) nano_core code.
// Basically, just searches for global (public) symbols, which are added to the system map and the crate metadata.
pub fn parse_nano_core(
    kernel_mmi: &mut MemoryManagementInfo, 
    text_pages:   MappedPages, 
    rodata_pages: MappedPages, 
    data_pages:   MappedPages, 
    log: bool
) -> Result<usize, &'static str> {
    debug!("parse_nano_core: trying to load and parse the nano_core file");
    let module = try!(get_module("__k_nano_core").ok_or("Couldn't find module called __k_nano_core"));
    use kernel_config::memory::address_is_page_aligned;
    if !address_is_page_aligned(module.start_address()) {
        error!("module {} is not page aligned!", module.name());
        return Err("nano_core module was not page aligned");
    } 

    // below, we map the nano_core module file just so we can parse it. We don't need to actually load it since we're already running it.
    if let PageTable::Active(ref mut active_table) = kernel_mmi.page_table {
        let (size, flags) = if PARSE_NANO_CORE_SYMBOL_FILE {
            (
                // + 1 to add space for appending a null character to the end of the symbol file string
                module.size() + 1, 
                // WRITABLE because we need to write that null character
                EntryFlags::PRESENT | EntryFlags::WRITABLE
            )
        }
        else {
            ( 
                module.size(), 
                EntryFlags::PRESENT 
            )
        };

        let temp_module_mapping = {
            let new_pages = try!(allocate_pages_by_bytes(size).ok_or("couldn't allocate pages for nano_core module"));
            let mut frame_allocator = try!(FRAME_ALLOCATOR.try().ok_or("couldn't get FRAME_ALLOCATOR")).lock();
            try!( active_table.map_allocated_pages_to(
                new_pages, Frame::range_inclusive_addr(module.start_address(), size), 
                flags, frame_allocator.deref_mut())
            )
        };

        let new_crate = if PARSE_NANO_CORE_SYMBOL_FILE {
            parse_nano_core_symbol_file(temp_module_mapping, text_pages, rodata_pages, data_pages, size)?
        } else {
            parse_nano_core_binary(temp_module_mapping, text_pages, rodata_pages, data_pages, size)?
        };

        let new_syms = add_crate(new_crate, log);
        info!("parsed nano_core crate, {} new symbols.", new_syms);
        Ok(new_syms)

        // temp_module_mapping is automatically unmapped when it falls out of scope here (frame allocator must not be locked)
    }
    else {
        error!("parse_nano_core(): error getting kernel's active page table to map module.");
        Err("couldn't get kernel's active page table")
    }
}



/// Parses the nano_core symbol file that represents the already loaded (and currently running) nano_core code.
/// Basically, just searches for global (public) symbols, which are added to the system map and the crate metadata.
/// 
/// Drops the given `mapped_pages` that hold the nano_core module file itself.
pub fn parse_nano_core_symbol_file(
    mapped_pages: MappedPages, 
    text_pages:   MappedPages, 
    rodata_pages: MappedPages, 
    data_pages:   MappedPages, 
    size: usize
) -> Result<Arc<RwLock<LoadedCrate>>, &'static str> {
    let crate_name = String::from("nano_core");
    debug!("Parsing nano_core symbols: size {:#x}({}), mapped_pages: {:?}, text_pages: {:?}, rodata_pages: {:?}, data_pages: {:?}", 
        size, size, mapped_pages, text_pages, rodata_pages, data_pages);

    let mut sections: Vec<StrongSectionRef> = Vec::new();

    // we create the new crate here so we can obtain references to it later
    let new_crate = Arc::new(RwLock::new(
        LoadedCrate {
            crate_name:   crate_name, 
            sections:     Vec::new(),
            text_pages:   None,
            rodata_pages: None,
            data_pages:   None,
        }
    ));
    
    // ensure that there's a null byte at the end
    {
        let null_byte: &mut u8 = try!(mapped_pages.as_type_mut(size - 1));
        *null_byte = 0u8;
    }

    // scoped to drop the borrow on mapped_pages through `bytes`
    {
        use util::c_str::CStr;
        let bytes = try!(mapped_pages.as_slice_mut(0, size));
        let symbol_cstr = try!( CStr::from_bytes_with_nul(bytes).map_err(|e| {
            error!("parse_nano_core_symbols(): error casting memory to CStr: {:?}", e);
            "FromBytesWithNulError occurred when casting nano_core symbol memory to CStr"
        }));
        let symbol_str = try!(symbol_cstr.to_str().map_err(|e| {
            error!("parse_nano_core_symbols(): error with CStr::to_str(): {:?}", e);
            "Utf8Error occurred when parsing nano_core symbols CStr"
        }));

        // debug!("========================= NANO_CORE SYMBOL STRING ========================\n{}", symbol_str);


        let mut text_shndx:   Option<usize> = None;
        let mut data_shndx:   Option<usize> = None;
        let mut rodata_shndx: Option<usize> = None;
        let mut bss_shndx:    Option<usize> = None;

        
        // a closure that parses a section index out of a string like "[7]"
        let parse_section_ndx = |str_ref: &str| {
            let open  = str_ref.find("[");
            let close = str_ref.find("]");
            open.and_then(|start| close.and_then(|end| str_ref.get((start + 1) .. end)))
                .and_then(|t| t.trim().parse::<usize>().ok())
        };

        // first, find the section indices that we care about: .text, .data, .rodata, and .bss
        let file_iterator = symbol_str.lines().enumerate();
        for (_line_num, line) in file_iterator.clone() {

            // skip empty lines
            let line = line.trim();
            if line.is_empty() { continue; }

            // debug!("Looking at line: {:?}", line);

            if line.contains(".text") && line.contains("PROGBITS") {
                text_shndx = parse_section_ndx(line);
            }
            else if line.contains(".data") && line.contains("PROGBITS") {
                data_shndx = parse_section_ndx(line);
            }
            else if line.contains(".rodata") && line.contains("PROGBITS") {
                rodata_shndx = parse_section_ndx(line);
            }
            else if line.contains(".bss") && line.contains("NOBITS") {
                bss_shndx = parse_section_ndx(line);
            }

            // once we've found the 4 sections we care about, we're done
            if text_shndx.is_some() && rodata_shndx.is_some() && data_shndx.is_some() && bss_shndx.is_some() {
                break;
            }
        }

        let text_shndx   = text_shndx.ok_or("parse_nano_core_symbols(): couldn't find .text section index")?;
        let rodata_shndx = rodata_shndx.ok_or("parse_nano_core_symbols(): couldn't find .rodata section index")?;
        let data_shndx   = data_shndx.ok_or("parse_nano_core_symbols(): couldn't find .data section index")?;
        let bss_shndx    = bss_shndx.ok_or("parse_nano_core_symbols(): couldn't find .bss section index")?;


        // second, skip ahead to the start of the symbol table 
        let mut file_iterator = file_iterator.skip_while( | (_line_num, line) |  {
            !line.starts_with("Symbol table")
        });
        // skip the symbol table start line, e.g., "Symbol table '.symtab' contains N entries:"
        if let Some((_num, _line)) = file_iterator.next() {
            // trace!("SKIPPING LINE {}: {}", _num + 1, _line);
        }
        // skip one more line, the line with the column headers, e.g., "Num:     Value     Size Type   Bind   Vis ..."
        if let Some((_num, _line)) = file_iterator.next() {
            // trace!("SKIPPING LINE {}: {}", _num + 1, _line);
        }


        // third, parse each symbol table entry, which should all have "GLOBAL" bindings
        for (_line_num, line) in file_iterator {
            if line.is_empty() { continue; }
            
            // we need the following items from a symbol table entry:
            // * Value (address),      column 1
            // * Size,                 column 2
            // * Ndx,                  column 6
            // * DemangledName#hash    column 7 to end

            // Can't use split_whitespace() here, because we need to splitn and then get the remainder of the line
            // after we've split the first 7 columns by whitespace. So we write a custom closure to group multiple whitespaces together.\
            // We use "splitn(8, ..)" because it stops at the 8th column (column index 7) and gets the rest of the line in a single iteration.
            let mut prev_whitespace = true; // by default, we start assuming that the previous element was whitespace.
            let mut parts = line.splitn(8, |c: char| {
                if c.is_whitespace() {
                    if prev_whitespace {
                        false
                    } else {
                        prev_whitespace = true;
                        true
                    }
                } else {
                    prev_whitespace = false;
                    false
                }
            }).map(str::trim);

            let _num         = parts.next().ok_or("parse_nano_core_symbols(): couldn't get column 0 'Num:'")?;
            let sec_vaddr    = parts.next().ok_or("parse_nano_core_symbols(): couldn't get column 1 'Value'")?;
            let sec_size     = parts.next().ok_or("parse_nano_core_symbols(): couldn't get column 2 'Size'")?;
            let _typ         = parts.next().ok_or("parse_nano_core_symbols(): couldn't get column 3 'Type'")?;
            let _bind        = parts.next().ok_or("parse_nano_core_symbols(): couldn't get column 4 'Bind'")?;
            let _vis         = parts.next().ok_or("parse_nano_core_symbols(): couldn't get column 5 'Vis'")?;
            let sec_ndx      = parts.next().ok_or("parse_nano_core_symbols(): couldn't get column 6 'Ndx'")?;
            let name_hash    = parts.next().ok_or("parse_nano_core_symbols(): couldn't get column 7 'Name'")?;

            // According to the operation of the tool "demangle_readelf_file", the last 'Name' column
            // consists of the already demangled name (which may have spaces) and then an optional hash,
            // which looks like the following:  NAME#HASH.
            // If there is no hash, then it will just be:   NAME
            // Thus, we need to split "name_hash"  at the '#', if it exists
            let (no_hash, hash) = {
                let mut tokens = name_hash.split("#");
                let no_hash = tokens.next().ok_or("parse_nano_core_symbols(): 'Name' column had extraneous '#' characters.")?;
                let hash = tokens.next();
                if tokens.next().is_some() {
                    error!("parse_nano_core_symbols(): 'Name' column \"{}\" had multiple '#' characters, expected only one as the hash separator!", name_hash);
                    return Err("parse_nano_core_symbols(): 'Name' column had multiple '#' characters, expected only one '#' as the hash separator!");
                }
                (no_hash.to_string(), hash.map(str::to_string))
            };
            
            let sec_vaddr = usize::from_str_radix(sec_vaddr, 16).map_err(|e| {
                error!("parse_nano_core_symbols(): error parsing virtual address Value at line {}: {:?}\n    line: {}", _line_num + 1, e, line);
                "parse_nano_core_symbols(): couldn't parse virtual address (value column)"
            })?; 
            let sec_size = usize::from_str_radix(sec_size, 10).map_err(|e| {
                error!("parse_nano_core_symbols(): error parsing size at line {}: {:?}\n    line: {}", _line_num + 1, e, line);
                "parse_nano_core_symbols(): couldn't parse size column"
            })?; 

            // while vaddr and size are required, ndx could be valid or not. 
            let sec_ndx = match usize::from_str_radix(sec_ndx, 10) {
                // If ndx is a valid number, proceed on. 
                Ok(ndx) => ndx,
                // Otherwise, if ndx is not a number (e.g., "ABS"), then we just skip that entry (go onto the next line). 
                _ => {
                    trace!("parse_nano_core_symbols(): skipping line {}: {}", _line_num + 1, line);
                    continue;
                }
            };

            // debug!("parse_nano_core_symbols(): name: {}, hash: {:?}, vaddr: {:#X}, size: {:#X}, sec_ndx {}", no_hash, hash, sec_vaddr, sec_size, sec_ndx);

            if sec_ndx == text_shndx {
                sections.push(
                    Arc::new(Mutex::new(LoadedSection::new(
                        SectionType::Text,
                        no_hash,
                        hash,
                        text_pages.offset_of_address(sec_vaddr).ok_or("nano_core text section wasn't covered by its mapped pages!")?,
                        sec_size,
                        true,
                        Arc::downgrade(&new_crate),
                    )))
                );
            }
            else if sec_ndx == rodata_shndx {
                sections.push(
                    Arc::new(Mutex::new(LoadedSection::new(
                        SectionType::Rodata,
                        no_hash,
                        hash,
                        rodata_pages.offset_of_address(sec_vaddr).ok_or("nano_core rodata section wasn't covered by its mapped pages!")?,
                        sec_size,
                        true,
                        Arc::downgrade(&new_crate),
                    )))
                );
            }
            else if (sec_ndx == data_shndx) || (sec_ndx == bss_shndx) {
                sections.push(
                    Arc::new(Mutex::new(LoadedSection::new(
                        SectionType::Data,
                        no_hash,
                        hash,
                        data_pages.offset_of_address(sec_vaddr).ok_or("nano_core data/bss section wasn't covered by its mapped pages!")?,
                        sec_size,
                        true,
                        Arc::downgrade(&new_crate),
                    )))
                );
            }
            else {
                trace!("parse_nano_core_symbols(): skipping sec[{}] (probably in .init): name: {}, vaddr: {:#X}, size: {:#X}", sec_ndx, no_hash, sec_vaddr, sec_size);
            }

        }

    } // drops the borrow of `bytes` (and mapped_pages)

    {
        let mut new_crate_locked = new_crate.write();
        new_crate_locked.sections     = sections;
        new_crate_locked.text_pages   = Some(text_pages);
        new_crate_locked.rodata_pages = Some(rodata_pages);
        new_crate_locked.data_pages   = Some(data_pages);
    }
    Ok(new_crate)
}




/// Parses the nano_core ELF binary file, which is already loaded and running.  
/// Thus, we simply search for its global symbols, and add them to the system map and the crate metadata.
/// 
/// Drops the given `mapped_pages` that hold the nano_core binary file itself.
fn parse_nano_core_binary(
    mapped_pages: MappedPages, 
    text_pages:   MappedPages, 
    rodata_pages: MappedPages, 
    data_pages:   MappedPages, 
    size_in_bytes: usize
) -> Result<Arc<RwLock<LoadedCrate>>, &'static str> {
    let crate_name = String::from("nano_core");
    debug!("Parsing {} binary: size {:#x}({}), MappedPages: {:?}, text_pages: {:?}, rodata_pages: {:?}, data_pages: {:?}", 
            crate_name, size_in_bytes, size_in_bytes, mapped_pages, text_pages, rodata_pages, data_pages);

    let byte_slice: &[u8] = mapped_pages.as_slice(0, size_in_bytes)?;
    // debug!("BYTE SLICE: {:?}", byte_slice);
    let elf_file = ElfFile::new(byte_slice)?; // returns Err(&str) if ELF parse fails

    // For us to properly load the ELF file, it must NOT have been stripped,
    // meaning that it must still have its symbol table section. Otherwise, relocations will not work.
    use xmas_elf::sections::SectionData::SymbolTable64;
    let sssec = elf_file.section_iter().filter(|sec| sec.get_type() == Ok(ShType::SymTab)).next();
    let symtab_data = match sssec.ok_or("no symtab section").and_then(|s| s.get_data(&elf_file)) {
        Ok(SymbolTable64(symtab)) => Ok(symtab),
        _ => {
            error!("parse_nano_core_binary(): can't load file: no symbol table found. Was file stripped?");
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
        // trace!("parse_nano_core_binary(): looking at sec[{}]: {:?}", shndx, sec);
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

        // // once we've found the 4 sections we care about, skip the rest.
        // if text_shndx.is_some() && rodata_shndx.is_some() && data_shndx.is_some() && bss_shndx.is_some() {
        //     break;
        // }
    }

    let text_shndx   = try!(text_shndx.ok_or("couldn't find .text section in nano_core ELF"));
    let rodata_shndx = try!(rodata_shndx.ok_or("couldn't find .rodata section in nano_core ELF"));
    let data_shndx   = try!(data_shndx.ok_or("couldn't find .data section in nano_core ELF"));
    let bss_shndx    = try!(bss_shndx.ok_or("couldn't find .bss section in nano_core ELF"));

    // we create the new crate here so we can obtain references to it later
    let new_crate = Arc::new(RwLock::new(
        LoadedCrate {
            crate_name:   crate_name, 
            sections:     Vec::new(),
            text_pages:   None,
            rodata_pages: None,
            data_pages:   None,
        }
    ));

    // iterate through the symbol table so we can find which sections are global (publicly visible)
    let loaded_sections = {
        let mut sections: Vec<StrongSectionRef> = Vec::new();
        use xmas_elf::symbol_table::Entry;
        for entry in symtab.iter() {
            // public symbols can have any visibility setting, but it's the binding that matters (must be GLOBAL)

            // use xmas_elf::symbol_table::Visibility;
            // match entry.get_other() {
            //     Visibility::Default | Visibility::Hidden => {
            //         // do nothing, fall through to proceed
            //     }
            //     _ => {
            //         continue; // skip this
            //     }
            // };
            
            if let Ok(bind) = entry.get_binding() {
                if bind == xmas_elf::symbol_table::Binding::Global {
                    if let Ok(typ) = entry.get_type() {
                        if typ == xmas_elf::symbol_table::Type::Func || typ == xmas_elf::symbol_table::Type::Object {
                            let sec_vaddr = entry.value() as VirtualAddress;
                            let sec_size = entry.size() as usize;
                            let name = entry.get_name(&elf_file)?;

                            let demangled = super::demangle_symbol(name);
                            // debug!("parse_nano_core_binary(): name: {}, demangled: {}, vaddr: {:#X}, size: {:#X}", name, demangled.no_hash, sec_vaddr, sec_size);

                            let new_section = {
                                if entry.shndx() as usize == text_shndx {
                                    Some(LoadedSection::new(
                                        SectionType::Text,
                                        demangled.no_hash,
                                        demangled.hash,
                                        try!(text_pages.offset_of_address(sec_vaddr).ok_or("nano_core text section wasn't covered by its mapped pages!")),
                                        sec_size,
                                        true,
                                        Arc::downgrade(&new_crate),
                                    ))
                                }
                                else if entry.shndx() as usize == rodata_shndx {
                                    Some(LoadedSection::new(
                                        SectionType::Rodata,
                                        demangled.no_hash,
                                        demangled.hash,
                                        try!(rodata_pages.offset_of_address(sec_vaddr).ok_or("nano_core rodata section wasn't covered by its mapped pages!")),
                                        sec_size,
                                        true,
                                        Arc::downgrade(&new_crate),
                                    ))
                                }
                                else if (entry.shndx() as usize == data_shndx) || (entry.shndx() as usize == bss_shndx) {
                                    Some(LoadedSection::new(
                                        SectionType::Data,
                                        demangled.no_hash,
                                        demangled.hash,
                                        try!(data_pages.offset_of_address(sec_vaddr).ok_or("nano_core data/bss section wasn't covered by its mapped pages!")),
                                        sec_size,
                                        true,
                                        Arc::downgrade(&new_crate),
                                    ))
                                }
                                else {
                                    error!("Unexpected entry.shndx(): {}", entry.shndx());
                                    None
                                }
                            };

                            if let Some(sec) = new_section {
                                // debug!("parse_nano_core: new section: {:?}", sec);
                                sections.push(Arc::new(Mutex::new(sec)));
                            }
                        }
                    }
                }
            }
        }   
        sections 
    };

    {
        let mut new_crate_locked = new_crate.write();
        new_crate_locked.sections     = loaded_sections;
        new_crate_locked.text_pages   = Some(text_pages);
        new_crate_locked.rodata_pages = Some(rodata_pages);
        new_crate_locked.data_pages   = Some(data_pages);
    }
    Ok(new_crate)
}
