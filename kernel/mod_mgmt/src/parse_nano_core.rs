//! Routines for parsing the `nano_core`, the fully-linked, already-loaded base kernel image,
//! in other words, the code that is currently executing.
//! As such, it performs no loading, but rather just creates metadata that represents
//! the existing kernel code that was loaded by the bootloader, and adds those functions to the system map.

use core::ops::Range;
use alloc::{
    string::{String, ToString},
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use spin::Mutex;
use cow_arc::{CowArc, CowWeak};
use xmas_elf::{
    self,
    ElfFile,
    sections::{ShType, SectionData, SHF_WRITE, SHF_ALLOC, SHF_EXECINSTR},
};
use rustc_demangle::demangle;
use cstr_core::CStr;
use memory::{VirtualAddress, MappedPages};
use crate_metadata::{LoadedCrate, StrongCrateRef, LoadedSection, StrongSectionRef, SectionType, Shndx};
use hashbrown::HashMap;
use path::Path;
use super::CrateNamespace;


/// The file name (without extension) that we expect to see in the namespace's kernel crate directory.
/// The trailing period '.' is there to avoid matching the "nano_core-<hash>.o" object file. 
const NANO_CORE_FILENAME_PREFIX: &str = "nano_core.";
const NANO_CORE_CRATE_NAME: &str = "nano_core";


/// Just like Rust's `try!()` macro, but packages up the given error message in a tuple
/// with the array of 3 MappedPages that must also be returned. 
macro_rules! try_mp {
    ($expr:expr, $tp:expr, $rp:expr, $dp:expr) => (match $expr {
        Ok(val) => val,
        Err(err_msg) => return Err((err_msg, [$tp, $rp, $dp])),
    });
}


/// Convenience function for calculating the address range of a MappedPages object.
fn mp_range(mp_ref: &Arc<Mutex<MappedPages>>) -> Range<VirtualAddress> {
    let mp = mp_ref.lock();
    mp.start_address() .. (mp.start_address() + mp.size_in_bytes())
}


/// Parses the nano_core object file that represents the already loaded (and currently running) nano_core code.
/// Basically, just searches for global (public) symbols, which are added to the system map and the crate metadata.
/// 
/// # Return
/// If successful, this returns a tuple of the following:
/// * `nano_core_crate_ref`: A reference to the newly-created nano_core crate.
/// * `init_symbols`: a map of symbol name to its constant value, which contains assembler and linker constances.
/// * The number of new symbols added to the symbol map (a `usize`).
/// 
/// If an error occurs, the returned `Result::Err` contains the passed-in `text_pages`, `rodata_pages`, and `data_pages`
/// because those cannot be dropped, as they hold the currently-running code, and dropping them would cause endless exceptions.
pub fn parse_nano_core(
    namespace:    &Arc<CrateNamespace>,
    text_pages:   MappedPages, 
    rodata_pages: MappedPages, 
    data_pages:   MappedPages, 
    verbose_log:  bool
) -> Result<(StrongCrateRef, BTreeMap<String, usize>, usize), (&'static str, [Arc<Mutex<MappedPages>>; 3])> {

    let text_pages   = Arc::new(Mutex::new(text_pages));
    let rodata_pages = Arc::new(Mutex::new(rodata_pages));
    let data_pages   = Arc::new(Mutex::new(data_pages));

    let (nano_core_file, real_namespace) = try_mp!(
        CrateNamespace::get_crate_object_file_starting_with(namespace, NANO_CORE_FILENAME_PREFIX)
            .ok_or("couldn't find the expected \"nano_core\" kernel file"),
        text_pages, rodata_pages, data_pages
    );
    let nano_core_file_path = Path::new(nano_core_file.lock().get_absolute_path());
    debug!("parse_nano_core: trying to load and parse the nano_core file: {:?}", nano_core_file_path);

    let crate_name = String::from(NANO_CORE_CRATE_NAME);

    // Create the LoadedCrate instance to represent the nano_core. 
    // It will be properly populated in one of the parse_nano_core_* functions below
    let nano_core_crate_ref = CowArc::new(LoadedCrate {
        crate_name:          crate_name.clone(),
        object_file:         nano_core_file.clone(),
        debug_symbols_file:  Arc::downgrade(&nano_core_file),
        sections:            HashMap::new(),
        text_pages:          Some((text_pages.clone(),   mp_range(&text_pages))),
        rodata_pages:        Some((rodata_pages.clone(), mp_range(&rodata_pages))),
        data_pages:          Some((data_pages.clone(),   mp_range(&data_pages))),
        global_sections:     BTreeSet::new(),
        data_sections:       BTreeSet::new(),
        reexported_symbols:  BTreeSet::new(),
    });

    // We don't need to actually load the nano_core as a new crate, since we're already running it.
    // We just need to parse it to discover the symbols. 
    let parse_result = match nano_core_file_path.extension() {
        Some("sym") => parse_nano_core_symbol_file(&nano_core_crate_ref, &text_pages, &rodata_pages, &data_pages),
        Some("bin") => parse_nano_core_binary(&nano_core_crate_ref, &text_pages, &rodata_pages, &data_pages),
        _ => Err("nano_core object file had unexpected file extension. Expected \".bin\" or \".sym\""),
    };
    let parsed_crate_items = try_mp!(parse_result, text_pages, rodata_pages, data_pages);

    let new_syms: usize;
    // Access and propertly set the new_crate's sections list and other items.
    {
        let mut new_crate_mut = try_mp!(
            nano_core_crate_ref.lock_as_mut()
                .ok_or_else(|| "BUG: parse_nano_core(): couldn't get exclusive mutable access to new_crate"),
            text_pages, rodata_pages, data_pages
        );

        trace!("parse_nano_core(): adding symbols to namespace {:?}...", real_namespace.name);
        new_syms = real_namespace.add_symbols(parsed_crate_items.sections.values(), verbose_log);
        trace!("parse_nano_core(): finished adding symbols.");

        new_crate_mut.sections        = parsed_crate_items.sections;
        new_crate_mut.global_sections = parsed_crate_items.global_sections;
        new_crate_mut.data_sections   = parsed_crate_items.data_sections;
    }

    // Add the newly-parsed nano_core crate to the kernel namespace.
    real_namespace.crate_tree.lock().insert(crate_name.into(), nano_core_crate_ref.clone_shallow());
    info!("Finished parsing nano_core crate, {} new symbols.", new_syms);
    Ok((nano_core_crate_ref, parsed_crate_items.init_symbols, new_syms))
}



/// Parses the nano_core symbol file that represents the already loaded (and currently running) nano_core code.
/// Basically, just searches the section list for offsets, size, and flag data,
/// and parses the symbol table to populate the list of sections.
fn parse_nano_core_symbol_file(
    new_crate_ref: &StrongCrateRef,
    text_pages:    &Arc<Mutex<MappedPages>>,
    rodata_pages:  &Arc<Mutex<MappedPages>>,
    data_pages:    &Arc<Mutex<MappedPages>>,
) -> Result<ParsedCrateItems, &'static str> {
    let new_crate_weak_ref = CowArc::downgrade(&new_crate_ref);
    let new_crate = new_crate_ref.lock_as_ref();
    let nano_core_object_file = new_crate.object_file.lock();
    let size = nano_core_object_file.size();
    let mapped_pages = nano_core_object_file.as_mapping()?;

    debug!("Parsing nano_core symbol file: size {:#x}({}), mapped_pages: {:?}, text_pages: {:?}, rodata_pages: {:?}, data_pages: {:?}", 
        size, size, mapped_pages, text_pages, rodata_pages, data_pages);

    let bytes = mapped_pages.as_slice(0, size)?;
    let symbol_cstr = CStr::from_bytes_with_nul(bytes).map_err(|e| {
        error!("parse_nano_core_symbol_file(): error casting nano_core symbol file to CStr: {:?}", e);
        "FromBytesWithNulError occurred when casting nano_core symbol file to CStr"
    })?;
    let symbol_str = symbol_cstr.to_str().map_err(|e| {
        error!("parse_nano_core_symbol_file(): error with CStr::to_str(): {:?}", e);
        "Utf8Error occurred when parsing nano_core symbols CStr"
    })?;

    let mut text_shndx:   Option<Shndx> = None;
    let mut data_shndx:   Option<Shndx> = None;
    let mut rodata_shndx: Option<Shndx> = None;
    let mut bss_shndx:    Option<Shndx> = None;

    // a closure that parses a section header's index (e.g., "[7]") out of the given str
    let parse_section_ndx = |str_ref: &str| {
        let open  = str_ref.find("[");
        let close = str_ref.find("]");
        open.and_then(|start| close.and_then(|end| str_ref.get((start + 1) .. end)))
            .and_then(|t| t.trim().parse::<usize>().ok())
    };

    // a closure that parses a section header's address and size
    let parse_section_vaddr_size = |sec_hdr_line_starting_at_name: &str| {
        let mut tokens = sec_hdr_line_starting_at_name.split_whitespace();
        tokens.next(); // skip Name 
        tokens.next(); // skip Type
        let addr_hex_str = tokens.next();
        tokens.next(); // skip Off (offset)
        let size_hex_str = tokens.next();
        // parse both the Address and Size fields as hex strings
        addr_hex_str.and_then(|a| usize::from_str_radix(a, 16).ok())
            .and_then(|addr| VirtualAddress::new(addr).ok())
            .and_then(|vaddr| {
                size_hex_str.and_then(|s| usize::from_str_radix(s, 16).ok())
                    .and_then(|size| Some((vaddr, size)))
            })
    };

    // We will fill in these crate items while parsing the symbol file.
    let mut crate_items = ParsedCrateItems::empty();
    // As the nano_core doesn't have one section per function/data/rodata, we fake it here with an arbitrary section counter
    let mut section_counter = 0;

    // First, find the section indices that we care about: .text, .data, .rodata, .bss, 
    // and also .eh_frame and .gcc_except_table, which are handled specially.
    // The reason we first look for the section indices is because we create
    // individual sections per symbol instead of one for each of those four sections,
    // which is how normal Rust crates are built and loaded (one section per symbol).
    let file_iterator = symbol_str.lines().enumerate();
    for (_line_num, line) in file_iterator.clone() {

        // skip empty lines
        let line = line.trim();
        if line.is_empty() { continue; }
        // debug!("Looking at line: {:?}", line);

        if line.contains(".text ") && line.contains("PROGBITS") {
            text_shndx = parse_section_ndx(line);
        }
        else if line.contains(".data ") && line.contains("PROGBITS") {
            data_shndx = parse_section_ndx(line);
        }
        else if line.contains(".rodata ") && line.contains("PROGBITS") {
            rodata_shndx = parse_section_ndx(line);
        }
        else if line.contains(".bss ") && line.contains("NOBITS") {
            bss_shndx = parse_section_ndx(line);
        }
        else if let Some(start) = line.find(".eh_frame ") {
            let (sec_vaddr, sec_size) = parse_section_vaddr_size(&line[start..])
                .ok_or("Failed to parse the .eh_frame section header's address and size")?;
            let mapped_pages_offset = rodata_pages.lock().offset_of_address(sec_vaddr)
                .ok_or("the nano_core .eh_frame section wasn't covered by the read-only mapped pages!")?;
            crate_items.sections.insert(
                section_counter,
                Arc::new(LoadedSection::new(
                    SectionType::EhFrame,
                    String::from(".eh_frame"),
                    Arc::clone(&rodata_pages),
                    mapped_pages_offset,
                    sec_vaddr,
                    sec_size,
                    false, // .eh_frame is not global
                    new_crate_weak_ref.clone(),
                ))
            );
            section_counter += 1;
        }
        else if let Some(start) = line.find(".gcc_except_table ") {
            let (sec_vaddr, sec_size) = parse_section_vaddr_size(&line[start..])
                .ok_or("Failed to parse the .gcc_except_table section header's address and size")?;
            let mapped_pages_offset = rodata_pages.lock().offset_of_address(sec_vaddr)
                .ok_or("the nano_core .gcc_except_table section wasn't covered by the read-only mapped pages!")?;
            crate_items.sections.insert(
                section_counter,
                Arc::new(LoadedSection::new(
                    SectionType::GccExceptTable,
                    String::from(".gcc_except_table"),
                    Arc::clone(&rodata_pages),
                    mapped_pages_offset,
                    sec_vaddr,
                    sec_size,
                    false, // .gcc_except_table is not global
                    new_crate_weak_ref.clone(),
                ))
            );
            section_counter += 1;
        }
    }

    let text_shndx   = text_shndx  .ok_or("parse_nano_core_symbol_file(): couldn't find .text section index")?;
    let rodata_shndx = rodata_shndx.ok_or("parse_nano_core_symbol_file(): couldn't find .rodata section index")?;
    let data_shndx   = data_shndx  .ok_or("parse_nano_core_symbol_file(): couldn't find .data section index")?;
    let bss_shndx    = bss_shndx   .ok_or("parse_nano_core_symbol_file(): couldn't find .bss section index")?;
    let shndxs = MainShndx { text_shndx, rodata_shndx, data_shndx, bss_shndx };

    // second, skip ahead to the start of the symbol table 
    let mut file_iterator = file_iterator.skip_while(|(_line_num, line)|  !line.starts_with("Symbol table"));
    // skip the symbol table start line, e.g., "Symbol table '.symtab' contains N entries:"
    if let Some((_num, _line)) = file_iterator.next() {
        // trace!("SKIPPING LINE {}: {}", _num + 1, _line);
    }
    // skip one more line, the line with the column headers, e.g., "Num:     Value     Size Type   Bind   Vis ..."
    if let Some((_num, _line)) = file_iterator.next() {
        // trace!("SKIPPING LINE {}: {}", _num + 1, _line);
    }

    {
        let text_pages_locked = text_pages.lock();
        let rodata_pages_locked = rodata_pages.lock();
        let data_pages_locked = data_pages.lock();

        // third, parse each symbol table entry
        for (_line_num, line) in file_iterator {
            if line.is_empty() { continue; }
            
            // we need the following items from a symbol table entry:
            // * Value (address),      column 1
            // * Size,                 column 2
            // * Bind (visibility),    column 4
            // * Ndx,                  column 6
            // * DemangledName#hash    column 7 to end

            // Can't use split_whitespace() here, because we need to splitn and then get the remainder of the line
            // after we've split the first 7 columns by whitespace. So we write a custom closure to group multiple whitespaces together.
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

            let _num      = parts.next().ok_or("parse_nano_core_symbol_file(): couldn't get column 0 'Num'")?;
            let sec_vaddr = parts.next().ok_or("parse_nano_core_symbol_file(): couldn't get column 1 'Value'")?;
            let sec_size  = parts.next().ok_or("parse_nano_core_symbol_file(): couldn't get column 2 'Size'")?;
            let _typ      = parts.next().ok_or("parse_nano_core_symbol_file(): couldn't get column 3 'Type'")?;
            let bind      = parts.next().ok_or("parse_nano_core_symbol_file(): couldn't get column 4 'Bind'")?;
            let _vis      = parts.next().ok_or("parse_nano_core_symbol_file(): couldn't get column 5 'Vis'")?;
            let sec_ndx   = parts.next().ok_or("parse_nano_core_symbol_file(): couldn't get column 6 'Ndx'")?;
            let name      = parts.next().ok_or("parse_nano_core_symbol_file(): couldn't get column 7 'Name'")?;
            
            let global = bind == "GLOBAL";
            let sec_vaddr = usize::from_str_radix(sec_vaddr, 16).map_err(|e| {
                error!("parse_nano_core_symbol_file(): error parsing virtual address Value at line {}: {:?}\n    line: {}", _line_num + 1, e, line);
                "parse_nano_core_symbol_file(): couldn't parse virtual address (value column)"
            })?;
            let sec_size = usize::from_str_radix(sec_size, 10).or_else(|e| {
                sec_size.get(2 ..).ok_or(e).and_then(|sec_size_hex| usize::from_str_radix(sec_size_hex, 16))
            }).map_err(|e| {
                error!("parse_nano_core_symbol_file(): error parsing size at line {}: {:?}\n    line: {}", _line_num + 1, e, line);
                "parse_nano_core_symbol_file(): couldn't parse size column"
            })?;

            // while vaddr and size are required, ndx could be valid or not. 
            let sec_ndx = match usize::from_str_radix(sec_ndx, 10) {
                // If ndx is a valid number, proceed on. 
                Ok(ndx) => ndx,
                // Otherwise, if ndx is not a number (e.g., "ABS"), then we just skip that entry (go onto the next line). 
                _ => {
                    trace!("parse_nano_core_symbol_file(): skipping line {}: {}", _line_num + 1, line);
                    continue;
                }
            };

            // debug!("parse_nano_core_symbol_file(): name: {}, vaddr: {:#X}, size: {:#X}, sec_ndx {}", name, sec_vaddr, sec_size, sec_ndx);

            add_new_section(
                &shndxs, 
                &mut crate_items, 
                text_pages, 
                rodata_pages, 
                data_pages, 
                &text_pages_locked,
                &rodata_pages_locked,
                &data_pages_locked,
                &new_crate_weak_ref,
                &mut section_counter,
                sec_ndx,
                String::from(name),
                sec_size,
                sec_vaddr,
                global
            )?;

        } // end of loop over all lines
    }

    trace!("parse_nano_core_symbol_file(): finished looping over symtab.");
    Ok(crate_items)
}




/// Parses the nano_core ELF binary file, which is already loaded and running.  
/// Thus, we simply search for its global symbols, and add them to the system map and the crate metadata.
/// 
/// Drops the given `mapped_pages` that hold the nano_core binary file itself.
fn parse_nano_core_binary(
    new_crate_ref: &StrongCrateRef,
    text_pages:    &Arc<Mutex<MappedPages>>,
    rodata_pages:  &Arc<Mutex<MappedPages>>,
    data_pages:    &Arc<Mutex<MappedPages>>,
) -> Result<ParsedCrateItems, &'static str> {
    let new_crate_weak_ref = CowArc::downgrade(&new_crate_ref);
    let new_crate = new_crate_ref.lock_as_ref();
    let nano_core_object_file = new_crate.object_file.lock();
    let size_in_bytes = nano_core_object_file.size();
    let mapped_pages = nano_core_object_file.as_mapping()?;

    debug!("Parsing nano_core binary: size {:#x}({}), MappedPages: {:?}, text_pages: {:?}, rodata_pages: {:?}, data_pages: {:?}", 
        size_in_bytes, size_in_bytes, mapped_pages, text_pages, rodata_pages, data_pages);

    let byte_slice: &[u8] = mapped_pages.as_slice(0, size_in_bytes)?;
    let elf_file = ElfFile::new(byte_slice)?; // returns Err(&str) if ELF parse fails

    // For us to properly load the ELF file, it must NOT have been stripped,
    // meaning that it must still have its symbol table section. Otherwise, relocations will not work.
    let sssec = elf_file.section_iter().filter(|sec| sec.get_type() == Ok(ShType::SymTab)).next();
    let symtab = match sssec.ok_or("no symtab section").and_then(|s| s.get_data(&elf_file)) {
        Ok(SectionData::SymbolTable64(symtab)) => symtab,
        _ => {
            error!("parse_nano_core_binary(): can't load file: no symbol table found. Was file stripped?");
            return Err("cannot load nano_core: no symbol table found. Was file stripped?");
        }
    };
    
    // find the .text, .data, and .rodata sections
    let mut text_shndx:   Option<Shndx> = None;
    let mut rodata_shndx: Option<Shndx> = None;
    let mut data_shndx:   Option<Shndx> = None;
    let mut bss_shndx:    Option<Shndx> = None;

    // We will fill in these crate items while parsing the symbol file.
    let mut crate_items = ParsedCrateItems::empty();
    // As the nano_core doesn't have one section per function/data/rodata, we fake it here with an arbitrary section counter
    let mut section_counter = 0;
    
    for (shndx, sec) in elf_file.section_iter().enumerate() {
        // trace!("parse_nano_core_binary(): looking at sec[{}]: {:?}", shndx, sec);
        // skip null section and any empty sections
        let sec_size = sec.size() as usize;
        if sec_size == 0 { continue; }
               
        match sec.get_name(&elf_file) {
            Ok(".text") => {
                if !(sec.flags() & (SHF_ALLOC | SHF_WRITE | SHF_EXECINSTR) == (SHF_ALLOC | SHF_EXECINSTR)) {
                    return Err(".text section had wrong flags!");
                }
                text_shndx = Some(shndx);
            }
            Ok(".rodata") => {
                if !(sec.flags() & (SHF_ALLOC | SHF_WRITE | SHF_EXECINSTR) == (SHF_ALLOC)) {
                    return Err(".rodata section had wrong flags!");
                }
                rodata_shndx = Some(shndx);
            }
            Ok(".data") => {
                if !(sec.flags() & (SHF_ALLOC | SHF_WRITE | SHF_EXECINSTR) == (SHF_ALLOC | SHF_WRITE)) {
                    return Err(".data section had wrong flags!");
                }
                data_shndx = Some(shndx);
            }
            Ok(".bss") => {
                if !(sec.flags() & (SHF_ALLOC | SHF_WRITE | SHF_EXECINSTR) == (SHF_ALLOC | SHF_WRITE)) {
                    return Err(".bss section had wrong flags!");
                }
                bss_shndx = Some(shndx);
            }
            Ok(".gcc_except_table") => {
                let sec_vaddr = VirtualAddress::new(sec.address() as usize)?;
                let mapped_pages_offset = rodata_pages.lock().offset_of_address(sec_vaddr)
                    .ok_or("the nano_core .gcc_except_table section wasn't covered by the read-only mapped pages!")?;
                crate_items.sections.insert(
                    section_counter,
                    Arc::new(LoadedSection::new(
                        SectionType::GccExceptTable,
                        String::from(".gcc_except_table"),
                        Arc::clone(&rodata_pages),
                        mapped_pages_offset,
                        sec_vaddr,
                        sec_size,
                        false, // .eh_frame is not global
                        new_crate_weak_ref.clone(),
                    ))
                );
                section_counter += 1;
            }
            Ok(".eh_frame") => {
                let sec_vaddr = VirtualAddress::new(sec.address() as usize)?;
                let mapped_pages_offset = rodata_pages.lock().offset_of_address(sec_vaddr)
                    .ok_or("the nano_core .eh_frame section wasn't covered by the read-only mapped pages!")?;
                crate_items.sections.insert(
                    section_counter,
                    Arc::new(LoadedSection::new(
                        SectionType::EhFrame,
                        String::from(".eh_frame"),
                        Arc::clone(&rodata_pages),
                        mapped_pages_offset,
                        sec_vaddr,
                        sec_size,
                        false, // .eh_frame is not global
                        new_crate_weak_ref.clone(),
                    ))
                );
                section_counter += 1;
            }
            _ => {
                continue;
            }
        }
    }

    let text_shndx   = text_shndx.ok_or("couldn't find .text section in nano_core ELF")?;
    let rodata_shndx = rodata_shndx.ok_or("couldn't find .rodata section in nano_core ELF")?;
    let data_shndx   = data_shndx.ok_or("couldn't find .data section in nano_core ELF")?;
    let bss_shndx    = bss_shndx.ok_or("couldn't find .bss section in nano_core ELF")?;
    let shndxs = MainShndx { text_shndx, rodata_shndx, data_shndx, bss_shndx };
    
    {
        let text_pages_locked = text_pages.lock();
        let rodata_pages_locked = rodata_pages.lock();
        let data_pages_locked = data_pages.lock();

        // Iterate through the symbol table so we can find which sections are global (publicly visible).
        use xmas_elf::symbol_table::Entry;
        for entry in symtab.iter() {
            // public symbols can have any visibility setting, but it's the binding that matters (GLOBAL or LOCAL)
            if let (Ok(bind), Ok(typ)) = (entry.get_binding(), entry.get_type()) {
                if typ == xmas_elf::symbol_table::Type::Func || typ == xmas_elf::symbol_table::Type::Object {
                    let sec_vaddr_value = entry.value() as usize;
                    let sec_size = entry.size() as usize;
                    let name = entry.get_name(&elf_file)?;
                    let global = bind == xmas_elf::symbol_table::Binding::Global;

                    let demangled = demangle(name).to_string();
                    // debug!("parse_nano_core_binary(): name: {}, demangled: {}, vaddr: {:#X}, size: {:#X}", name, demangled, sec_value, sec_size);

                    add_new_section(
                        &shndxs, 
                        &mut crate_items, 
                        text_pages, 
                        rodata_pages, 
                        data_pages, 
                        &text_pages_locked,
                        &rodata_pages_locked,
                        &data_pages_locked,
                        &new_crate_weak_ref,
                        &mut section_counter,
                        entry.shndx() as usize,
                        demangled,
                        sec_size,
                        sec_vaddr_value,
                        global
                    )?;
                }
            }
        }
    }

    Ok(crate_items)
}


/// The collection of sections and symbols obtained while parsing the nano_core crate.
struct ParsedCrateItems {
    sections:        HashMap<Shndx, StrongSectionRef>,
    global_sections: BTreeSet<Shndx>,
    data_sections:   BTreeSet<Shndx>,
    // The set of other non-section symbols too, such as constants defined in assembly code.
    init_symbols:    BTreeMap<String, usize>,
}
impl ParsedCrateItems {
    fn empty() -> ParsedCrateItems {
        ParsedCrateItems {
            sections:        HashMap::new(),
            global_sections: BTreeSet::new(),
            data_sections:   BTreeSet::new(),
            init_symbols:    BTreeMap::new(),
        }
    }
}


/// The section header indices (shndx) for the main sections:
/// .text, .rodata, .data, and .bss.
struct MainShndx {
    text_shndx:   Shndx,
    rodata_shndx: Shndx,
    data_shndx:   Shndx,
    bss_shndx:    Shndx,
}

/// A convenience function that separates out the logic 
/// of actually creating and adding a new LoadedSection instance
/// after it has been parsed. 
fn add_new_section(
    shndxs:              &MainShndx,
    crate_items:         &mut ParsedCrateItems,
    text_pages:          &Arc<Mutex<MappedPages>>,
    rodata_pages:        &Arc<Mutex<MappedPages>>,
    data_pages:          &Arc<Mutex<MappedPages>>,
    text_pages_locked:   &MappedPages,
    rodata_pages_locked: &MappedPages,
    data_pages_locked:   &MappedPages,
    new_crate_weak_ref:  &CowWeak<LoadedCrate>,
    section_counter:     &mut Shndx,
    // crate-wide args above, section-specific stuff below
    sec_ndx: Shndx,
    sec_name: String,
    sec_size: usize,
    sec_vaddr: usize,
    global: bool,
) -> Result<(), &'static str> {
    let new_section = if sec_ndx == shndxs.text_shndx {
        let sec_vaddr = VirtualAddress::new(sec_vaddr)?;
        Some(LoadedSection::new(
            SectionType::Text,
            sec_name,
            Arc::clone(&text_pages),
            text_pages_locked.offset_of_address(sec_vaddr).ok_or("nano_core text section wasn't covered by its mapped pages!")?,
            sec_vaddr,
            sec_size,
            global,
            new_crate_weak_ref.clone(), 
        ))
    }
    else if sec_ndx == shndxs.rodata_shndx {
        let sec_vaddr = VirtualAddress::new(sec_vaddr)?;
        Some(LoadedSection::new(
            SectionType::Rodata,
            sec_name,
            Arc::clone(&rodata_pages),
            rodata_pages_locked.offset_of_address(sec_vaddr).ok_or("nano_core rodata section wasn't covered by its mapped pages!")?,
            sec_vaddr,
            sec_size,
            global,
            new_crate_weak_ref.clone(),
        ))
    }
    else if sec_ndx == shndxs.data_shndx {
        let sec_vaddr = VirtualAddress::new(sec_vaddr)?;
        Some(LoadedSection::new(
            SectionType::Data,
            sec_name,
            Arc::clone(&data_pages),
            data_pages_locked.offset_of_address(sec_vaddr).ok_or("nano_core data section wasn't covered by its mapped pages!")?,
            sec_vaddr,
            sec_size,
            global,
            new_crate_weak_ref.clone(),
        ))
    }
    else if sec_ndx == shndxs.bss_shndx {
        let sec_vaddr = VirtualAddress::new(sec_vaddr)?;
        Some(LoadedSection::new(
            SectionType::Bss,
            sec_name,
            Arc::clone(&data_pages),
            data_pages_locked.offset_of_address(sec_vaddr).ok_or("nano_core bss section wasn't covered by its mapped pages!")?,
            sec_vaddr,
            sec_size,
            global,
            new_crate_weak_ref.clone(),
        ))
    }
    else {
        crate_items.init_symbols.insert(sec_name, sec_vaddr);
        None
    };

    if let Some(sec) = new_section {
        // debug!("parse_nano_core: new section: {:?}", sec);
        let sec_ref = Arc::new(sec);
        if sec_ref.global {
            crate_items.global_sections.insert(*section_counter);
        }
        if sec_ref.typ.is_data_or_bss() {
            crate_items.data_sections.insert(*section_counter);
        }
        crate_items.sections.insert(*section_counter, sec_ref);
        *section_counter += 1;
    }

    Ok(())
}