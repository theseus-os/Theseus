#![no_std]
#![feature(alloc)]

#[macro_use] extern crate alloc;
#[macro_use] extern crate lazy_static;
#[macro_use] extern crate log;
extern crate spin;
extern crate irq_safety;
extern crate xmas_elf;
extern crate memory;
extern crate kernel_config;
extern crate goblin;
extern crate util;
extern crate rustc_demangle;


use core::ops::DerefMut;
use alloc::{Vec, BTreeMap, BTreeSet, String};
use alloc::arc::Arc;
use alloc::string::ToString;
use spin::{Mutex, RwLock};

use xmas_elf::ElfFile;
use xmas_elf::sections::{SectionHeader, SectionData, ShType};
use xmas_elf::sections::{SHF_WRITE, SHF_ALLOC, SHF_EXECINSTR};
use goblin::elf::reloc::*;

use util::round_up_power_of_two;
use memory::{FRAME_ALLOCATOR, get_module, VirtualMemoryArea, MemoryManagementInfo, ModuleArea, Frame, PageTable, VirtualAddress, MappedPages, EntryFlags, allocate_pages_by_bytes};


pub mod metadata;
use self::metadata::{LoadedCrate, TextSection, DataSection, RodataSection, LoadedSection, StrongSectionRef, RelocationDependency};

// Can also try this crate: https://crates.io/crates/goblin
// ELF RESOURCE: http://www.cirosantilli.com/elf-hello-world


#[derive(PartialEq)]
pub enum CrateType {
    KernelModule,
    ApplicationModule,
    UserspaceModule,
}
impl CrateType {
    pub fn prefix(&self) -> &'static str {
        match self {
            CrateType::KernelModule       => "__k_",
            CrateType::ApplicationModule  => "__a_",
            CrateType::UserspaceModule    => "__u_",
        }
    }

    /// Returns a tuple of (CrateType, &str) based on the given `module_name`,
    /// in which the `&str` is the rest of the module name after the prefix. 
    /// # Examples 
    /// ```
    /// let result = CrateType::from_module_name("__k_my_crate");
    /// assert_eq!(result, (CrateType::KernelModule, "my_crate") );
    /// ```
    pub fn from_module_name<'a>(module_name: &'a str) -> Result<(CrateType, &'a str), &'static str> {
        if module_name.starts_with(CrateType::ApplicationModule.prefix()) {
            Ok((
                CrateType::ApplicationModule,
                module_name.get(CrateType::ApplicationModule.prefix().len() .. ).ok_or("Couldn't get name of application module")?
            ))
        }
        else if module_name.starts_with(CrateType::KernelModule.prefix()) {
            Ok((
                CrateType::KernelModule,
                module_name.get(CrateType::KernelModule.prefix().len() .. ).ok_or("Couldn't get name of kernel module")?
            ))
        }
        else if module_name.starts_with(CrateType::UserspaceModule.prefix()) {
            Ok((
                CrateType::UserspaceModule,
                module_name.get(CrateType::UserspaceModule.prefix().len() .. ).ok_or("Couldn't get name of userspace module")?
            ))
        }
        else {
            Err("module_name didn't start with a known CrateType prefix")
        }
    }


    pub fn is_application(module_name: &str) -> bool {
        module_name.starts_with(CrateType::ApplicationModule.prefix())
    }

    pub fn is_kernel(module_name: &str) -> bool {
        module_name.starts_with(CrateType::KernelModule.prefix())
    }

    pub fn is_userspace(module_name: &str) -> bool {
        module_name.starts_with(CrateType::UserspaceModule.prefix())
    }
}



pub struct ElfProgramSegment {
    /// the VirtualMemoryAddress that will represent the virtual mapping of this Program segment.
    /// Provides starting virtual address, size in memory, mapping flags, and a text description.
    pub vma: VirtualMemoryArea,
    /// the offset of this segment into the file.
    /// This plus the physical address of the Elf file is the physical address of this Program segment.
    pub offset: usize,
}


/// Parses an elf executable file as a slice of bytes starting at the given `MappedPages` mapping.
/// Consumes the given `MappedPages`, which automatically unmaps it at the end of this function. 
pub fn parse_elf_executable(mapped_pages: MappedPages, size_in_bytes: usize) -> Result<(Vec<ElfProgramSegment>, VirtualAddress), &'static str> {
    debug!("Parsing Elf executable: mapped_pages {:?}, size_in_bytes {:#x}({})", mapped_pages, size_in_bytes, size_in_bytes);

    let byte_slice: &[u8] = try!(mapped_pages.as_slice(0, size_in_bytes));
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






/// Loads the specified kernel crate into memory, allowing it to be invoked.  
/// Returns a Result containing the number of symbols that were added to the system map
/// as a result of loading this crate.
pub fn load_kernel_crate(module: &ModuleArea, kernel_mmi: &mut MemoryManagementInfo, log: bool) -> Result<usize, &'static str> {
    debug!("load_kernel_crate: trying to load \"{}\" kernel module", module.name());
    
    if module.name() == "__k_nano_core" {
        error!("load_kernel_crate() cannot be used for the nano_core, because the nano_core is already loaded. (call parse_nano_core() instead)");
        return Err("load_kernel_crate() cannot be used for the nano_core, because the nano_core is already loaded. (call parse_nano_core() instead)");
    }
    
    use kernel_config::memory::address_is_page_aligned;
    if !address_is_page_aligned(module.start_address()) {
        error!("module {} is not page aligned!", module.name());
        return Err("module was not page aligned");
    } 



    let size = module.size();

    // first we need to map the module memory region into our address space, 
    // so we can then parse the module as an ELF file in the kernel.
    let temp_module_mapping = {
        if let PageTable::Active(ref mut active_table) = kernel_mmi.page_table {
            let flags = EntryFlags::PRESENT;

            let new_pages = allocate_pages_by_bytes(size).ok_or("couldn't allocate pages for crate module")?;
            let mut frame_allocator = FRAME_ALLOCATOR.try().ok_or("couldn't get FRAME_ALLOCATOR")?.lock();
            active_table.map_allocated_pages_to(
                new_pages, Frame::range_inclusive_addr(module.start_address(), size), 
                flags, frame_allocator.deref_mut()
            )?
        }
        else {
            error!("load_kernel_crate(): error getting kernel's active page table to temporarily map module.");
            return Err("couldn't get kernel's active page table");
        }
    };

    let new_crate = parse_elf_kernel_crate(temp_module_mapping, size, module.name(), kernel_mmi, log)?;
    let new_syms = metadata::add_crate(new_crate, log);
    info!("loaded new crate module: {}, {} new symbols.", module.name(), new_syms);
    Ok(new_syms)
    
    // temp_module_mapping is automatically unmapped when it falls out of scope here (frame allocator must not be locked)
}




/// Loads the specified application crate into memory, allowing it to be invoked.  
/// Returns a Result containing the new crate.
pub fn load_application_crate(module: &ModuleArea, kernel_mmi: &mut MemoryManagementInfo, log: bool) 
    -> Result<Arc<RwLock<LoadedCrate>>, &'static str> 
{
    if !CrateType::is_application(module.name()) {
        error!("load_application_crate() cannot be used for module \"{}\", only for application modules starting with \"{}\"",
            module.name(), CrateType::ApplicationModule.prefix());
        return Err("load_application_crate() can only be used for application modules");
    }
    
    use kernel_config::memory::address_is_page_aligned;
    if !address_is_page_aligned(module.start_address()) {
        error!("module {} was not page aligned!", module.name());
        return Err("module was not page aligned");
    } 
    
    debug!("load_application_crate: trying to load \"{}\" application module", module.name());



    let size = module.size();

    // first we need to map the module memory region into our address space, 
    // so we can then parse the module as an ELF file in the kernel.
    let temp_module_mapping = {
        if let PageTable::Active(ref mut active_table) = kernel_mmi.page_table {
            let flags = EntryFlags::PRESENT;

            let new_pages = allocate_pages_by_bytes(size).ok_or("couldn't allocate pages for crate module")?;
            let mut frame_allocator = FRAME_ALLOCATOR.try().ok_or("couldn't get FRAME_ALLOCATOR")?.lock();
            active_table.map_allocated_pages_to(
                new_pages, Frame::range_inclusive_addr(module.start_address(), size), 
                flags, frame_allocator.deref_mut()
            )?
        }
        else {
            error!("load_application_crate(): error getting kernel's active page table to temporarily map module.");
            return Err("load_application_crate(): couldn't get kernel's active page table");
        }
    };

    let new_crate = parse_elf_kernel_crate(temp_module_mapping, size, module.name(), kernel_mmi, log)?;
    { 
        let new_crate_locked = new_crate.read(); 
        info!("loaded new application crate module: {}, num sections: {}", new_crate_locked.crate_name, new_crate_locked.sections.len());
    }
    Ok(new_crate)

    // temp_module_mapping is automatically unmapped when it falls out of scope here (frame allocator must not be locked)
}




/// A representation of a demangled symbol.
/// # Example
/// mangled:            "_ZN7console4init17h71243d883671cb51E"
/// demangled.no_hash:  "console::init"
/// demangled.hash:     "h71243d883671cb51E"
struct DemangledSymbol {
    no_hash: String, 
    hash: Option<String>,
}

fn demangle_symbol(s: &str) -> DemangledSymbol {
    use rustc_demangle::demangle;
    let demangled = demangle(s);
    let without_hash: String = format!("{:#}", demangled); // the fully-qualified symbol, no hash
    let with_hash: String = format!("{}", demangled); // the fully-qualified symbol, with the hash
    let hash_only: Option<String> = with_hash.find::<&str>(without_hash.as_ref())
        .and_then(|index| {
            let hash_start = index + 2 + without_hash.len();
            with_hash.get(hash_start..).map(|s| s.to_string())
        }); // + 2 to skip the "::" separator
    
    DemangledSymbol {
        no_hash: without_hash,
        hash: hash_only,
    }
}



/// The primary internal routine for parsing, loading, and linking a crate's module object file.
fn parse_elf_kernel_crate(mapped_pages: MappedPages, 
                          size_in_bytes: usize, 
                          module_name: &String, 
                          kernel_mmi: &mut MemoryManagementInfo, 
                          log: bool)
                          -> Result<Arc<RwLock<LoadedCrate>>, &'static str>
{
    
    let (_crate_type, crate_name) = CrateType::from_module_name(module_name)?;
    let crate_name = String::from(crate_name);
    debug!("Parsing Elf kernel crate: {:?}, size {:#x}({})", module_name, size_in_bytes, size_in_bytes);

    let byte_slice: &[u8] = try!(mapped_pages.as_slice(0, size_in_bytes));
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
                        _ => { }
                    }
                }
            }
        }   
        globals 
    };

    // Calculate how many bytes (and thus how many pages) we need for each of the three section types,
    // which are text (present | exec), rodata (present | noexec), data/bss (present | writable)
    let (text_bytecount, rodata_bytecount, data_bytecount): (usize, usize, usize) = {
        let (mut text, mut rodata, mut data) = (0, 0, 0);
        for sec in elf_file.section_iter() {
            let sec_typ = sec.get_type();
            // look for .text, .rodata, .data, and .bss sections
            if sec_typ == Ok(ShType::ProgBits) || sec_typ == Ok(ShType::NoBits) {
                let size = sec.size() as usize;
                if (size == 0) || (sec.flags() & SHF_ALLOC == 0) {
                    continue; // skip non-allocated sections (they're useless)
                }

                let align = sec.align() as usize;
                let addend = round_up_power_of_two(size, align);

                // filter flags for ones we care about (we already checked that it's loaded (SHF_ALLOC))
                let write: bool = sec.flags() & SHF_WRITE     == SHF_WRITE;
                let exec:  bool = sec.flags() & SHF_EXECINSTR == SHF_EXECINSTR;
                if exec {
                    // trace!("  Looking at sec with size {:#X} align {:#X} --> addend {:#X}", size, align, addend);
                    text += addend;
                }
                else if write {
                    // .bss sections have the same flags (write and alloc) as data, so combine them
                    data += addend;
                }
                else {
                    rodata += addend;
                }
            }
        }
        (text, rodata, data)
    };

    if log {
        debug!("    crate {} needs {:#X} text bytes, {:#X} rodata bytes, {:#X} data bytes", module_name, text_bytecount, rodata_bytecount, data_bytecount);
    }

    // create a closure here to allocate N contiguous virtual memory pages
    // and map them to random frames as writable, returns Result<MappedPages, &'static str>
    let (text_pages, rodata_pages, data_pages): (Option<Arc<MappedPages>>, Option<Arc<MappedPages>>, Option<Arc<MappedPages>>) = {
        use memory::FRAME_ALLOCATOR;
        let mut frame_allocator = try!(FRAME_ALLOCATOR.try().ok_or("couldn't get FRAME_ALLOCATOR")).lock();

        let mut allocate_pages_closure = |size_in_bytes: usize| {
            let allocated_pages = try!(allocate_pages_by_bytes(size_in_bytes).ok_or("Couldn't allocate_pages_by_bytes, out of virtual address space"));

            if let PageTable::Active(ref mut active_table) = kernel_mmi.page_table {
                // Right now we're just simply copying small sections to the new memory,
                // so we have to map those pages to real (randomly chosen) frames first. 
                // because we're copying bytes to the newly allocated pages, we need to make them writable too, 
                // and then change the page permissions (by using remap) later. 
                active_table.map_allocated_pages(allocated_pages, EntryFlags::PRESENT | EntryFlags::WRITABLE, frame_allocator.deref_mut())
            }
            else {
                return Err("couldn't get kernel's active page table");
            }

        };

        // we must allocate these pages separately because they will have different flags later
        (
            if text_bytecount   > 0 { Some( Arc::new( try!( allocate_pages_closure(text_bytecount))))   } else { None }, 
            if rodata_bytecount > 0 { Some( Arc::new( try!( allocate_pages_closure(rodata_bytecount)))) } else { None }, 
            if data_bytecount   > 0 { Some( Arc::new( try!( allocate_pages_closure(data_bytecount))))   } else { None }
        )
    };
    

    // First, we need to parse all the sections and load the text and data sections
    let mut loaded_sections: BTreeMap<usize, StrongSectionRef> = BTreeMap::new(); // map section header index (shndx) to LoadedSection

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

    let mut text_offset:   usize = 0;
    let mut rodata_offset: usize = 0;
    let mut data_offset:   usize = 0;
                
    const TEXT_PREFIX:   &'static str = ".text.";
    const RODATA_PREFIX: &'static str = ".rodata.";
    const DATA_PREFIX:   &'static str = ".data.";
    const BSS_PREFIX:    &'static str = ".bss.";
    const RELRO_PREFIX:  &'static str = "rel.ro.";


    for (shndx, sec) in elf_file.section_iter().enumerate() {
        // the PROGBITS sections (.text, .rodata, .data) and the NOBITS (.bss) sections are what we care about
        let sec_typ = sec.get_type();
        // look for PROGBITS (.text, .rodata, .data) and NOBITS (.bss) sections
        if sec_typ == Ok(ShType::ProgBits) || sec_typ == Ok(ShType::NoBits) {

            // even if we're using the next section's data (for a zero-sized section),
            // we still want to use this current section's actual name and flags!
            let sec_flags = sec.flags();
            let sec_name = match sec.get_name(&elf_file) {
                Ok(name) => name,
                Err(_e) => {
                    error!("parse_elf_kernel_crate: couldn't get section name for section [{}]: {:?}\n    error: {}", shndx, sec, _e);
                    return Err("couldn't get section name");
                }
            };

            
            // some special sections are fine to ignore
            if  sec_name.starts_with(".note")   ||   // ignore GNU note sections
                sec_name.starts_with(".gcc")    ||   // ignore gcc special sections for now
                sec_name.starts_with(".debug")  ||   // ignore debug special sections for now
                sec_name == ".text"                  // ignore the header .text section (with no content)
            {
                continue;    
            }            


            let sec = if sec.size() == 0 {
                // This is a very rare case of a zero-sized section. 
                // A section of size zero shouldn't necessarily be removed, as they are sometimes referenced in relocations,
                // typically the zero-sized section itself is a reference to the next section in the list of section headers).
                // Thus, we need to use the *current* section's name with the *next* section's (the next section's) information,
                // i.e., its  size, alignment, and actual data
                match elf_file.section_header((shndx + 1) as u16) { // get the next section
                    Ok(sec_hdr) => {
                        // The next section must have the same offset as the current zero-sized one
                        if sec_hdr.offset() == sec.offset() {
                            // if it does, we can use it in place of the current section
                            sec_hdr
                        }
                        else {
                            // if it does not, we should NOT use it in place of the current section
                            sec
                        }
                    }
                    _ => {
                        error!("parse_elf_kernel_crate(): Couldn't get next section for zero-sized section {}", shndx);
                        return Err("couldn't get next section for a zero-sized section");
                    }
                }
            }
            else {
                // this is the normal case, a non-zero sized section, so just use the current section
                sec
            };

            // get the relevant section info, i.e., size, alignment, and data contents
            let sec_size  = sec.size()  as usize;
            let sec_align = sec.align() as usize;


            if sec_name.starts_with(TEXT_PREFIX) {
                if let Some(name) = sec_name.get(TEXT_PREFIX.len() ..) {
                    let demangled = demangle_symbol(name);
                    if log { trace!("Found [{}] .text section: name {:?}, with_hash {:?}, size={:#x}", shndx, name, demangled.no_hash, sec_size); }
                    if sec_flags & (SHF_ALLOC | SHF_WRITE | SHF_EXECINSTR) != (SHF_ALLOC | SHF_EXECINSTR) {
                        error!(".text section [{}], name: {:?} had the wrong flags {:#X}", shndx, name, sec_flags);
                        return Err(".text section had wrong flags!");
                    }

                    if let Some(ref tp) = text_pages {
                        // here: we're ready to copy the text section to the proper address
                        let dest_slice: &mut [u8]  = try!(tp.as_slice_mut(text_offset, sec_size));
                        if log { 
                            let dest_addr = dest_slice as *mut [u8] as *mut u8 as VirtualAddress;
                            trace!("       dest_addr: {:#X}, text_offset: {:#X}", dest_addr, text_offset); 
                        }
                        match sec.get_data(&elf_file) {
                            Ok(SectionData::Undefined(sec_data)) => dest_slice.copy_from_slice(sec_data),
                            Ok(SectionData::Empty) => {
                                for b in dest_slice {
                                    *b = 0;
                                }
                            },
                            _ => {
                                error!("parse_elf_kernel_crate(): Couldn't get section data for .text section [{}] {}: {:?}", shndx, sec_name, sec.get_data(&elf_file));
                                return Err("couldn't get section data in .text section");
                            }
                        }
            
                        loaded_sections.insert(shndx, 
                            Arc::new(Mutex::new(LoadedSection::Text(TextSection{
                                abs_symbol: demangled.no_hash,
                                hash: demangled.hash,
                                mapped_pages: Arc::downgrade(tp),
                                mapped_pages_offset: text_offset,
                                size: sec_size,
                                global: global_sections.contains(&shndx),
                                parent_crate: Arc::downgrade(&new_crate),
                            })))
                        );

                        text_offset += round_up_power_of_two(sec_size, sec_align);
                    }
                    else {
                        return Err("no text_pages were allocated");
                    }
                }
                else {
                    error!("Failed to get the .text section's name after \".text.\": {:?}", sec_name);
                    return Err("Failed to get the .text section's name after \".text.\"!");
                }
            }

            else if sec_name.starts_with(RODATA_PREFIX) {
                if let Some(name) = sec_name.get(RODATA_PREFIX.len() ..) {
                    let demangled = demangle_symbol(name);
                    if log { trace!("Found [{}] .rodata section: name {:?}, demangled {:?}, size={:#x}", shndx, name, demangled.no_hash, sec_size); }
                    if sec_flags & (SHF_ALLOC | SHF_WRITE | SHF_EXECINSTR) != (SHF_ALLOC) {
                        error!(".rodata section [{}], name: {:?} had the wrong flags {:#X}", shndx, name, sec_flags);
                        return Err(".rodata section had wrong flags!");
                    }

                    if let Some(ref rp) = rodata_pages {
                        // here: we're ready to copy the rodata section to the proper address
                        let dest_slice: &mut [u8]  = try!(rp.as_slice_mut(rodata_offset, sec_size));
                        if log { 
                            let dest_addr = dest_slice as *mut [u8] as *mut u8 as VirtualAddress;
                            trace!("       dest_addr: {:#X}, rodata_offset: {:#X}", dest_addr, rodata_offset); 
                        }
                        match sec.get_data(&elf_file) {
                            Ok(SectionData::Undefined(sec_data)) => dest_slice.copy_from_slice(sec_data),
                            Ok(SectionData::Empty) => {
                                for b in dest_slice {
                                    *b = 0;
                                }
                            },
                            _ => {
                                error!("parse_elf_kernel_crate(): Couldn't get section data for .rodata section [{}] {}: {:?}", shndx, sec_name, sec.get_data(&elf_file));
                                return Err("couldn't get section data in .rodata section");
                            }
                        }

                        loaded_sections.insert(shndx, 
                            Arc::new(Mutex::new(LoadedSection::Rodata(RodataSection{
                                abs_symbol: demangled.no_hash,
                                hash: demangled.hash,
                                mapped_pages: Arc::downgrade(rp),
                                mapped_pages_offset: rodata_offset,
                                size: sec_size,
                                global: global_sections.contains(&shndx),
                                parent_crate: Arc::downgrade(&new_crate),
                            })))
                        );

                        rodata_offset += round_up_power_of_two(sec_size, sec_align);
                    }
                    else {
                        return Err("no rodata_pages were allocated");
                    }
                }
                else {
                    error!("Failed to get the .rodata section's name after \".rodata.\": {:?}", sec_name);
                    return Err("Failed to get the .rodata section's name after \".rodata.\"!");
                }
            }

            else if sec_name.starts_with(DATA_PREFIX) {
                if let Some(name) = sec_name.get(DATA_PREFIX.len() ..) {
                    let name = if name.starts_with(RELRO_PREFIX) {
                        let relro_name = try!(name.get(RELRO_PREFIX.len() ..).ok_or("Couldn't get name of .data.rel.ro. section"));
                        relro_name
                    }
                    else {
                        name
                    };
                    let demangled = demangle_symbol(name);
                    if log { trace!("Found [{}] .data section: name {:?}, with_hash {:?}, size={:#x}", shndx, name, demangled.no_hash, sec_size); }
                    if sec_flags & (SHF_ALLOC | SHF_WRITE | SHF_EXECINSTR) != (SHF_ALLOC | SHF_WRITE) {
                        error!(".data section [{}], name: {:?} had the wrong flags {:#X}", shndx, name, sec_flags);
                        return Err(".data section had wrong flags!");
                    }
                    
                    if let Some(ref dp) = data_pages {
                        // here: we're ready to copy the data/bss section to the proper address
                        let dest_slice: &mut [u8]  = try!(dp.as_slice_mut(data_offset, sec_size));
                        if log { 
                            let dest_addr = dest_slice as *mut [u8] as *mut u8 as VirtualAddress;
                            trace!("       dest_addr: {:#X}, data_offset: {:#X}", dest_addr, data_offset); 
                        }
                        match sec.get_data(&elf_file) {
                            Ok(SectionData::Undefined(sec_data)) => dest_slice.copy_from_slice(sec_data),
                            Ok(SectionData::Empty) => {
                                for b in dest_slice {
                                    *b = 0;
                                }
                            },
                            _ => {
                                error!("parse_elf_kernel_crate(): Couldn't get section data for .data section [{}] {}: {:?}", shndx, sec_name, sec.get_data(&elf_file));
                                return Err("couldn't get section data in .data section");
                            }
                        }

                        loaded_sections.insert(shndx, 
                            Arc::new(Mutex::new(LoadedSection::Data(DataSection{
                                abs_symbol: demangled.no_hash,
                                hash: demangled.hash,
                                mapped_pages: Arc::downgrade(dp),
                                mapped_pages_offset: data_offset,
                                size: sec_size,
                                global: global_sections.contains(&shndx),
                                parent_crate: Arc::downgrade(&new_crate),
                            })))
                        );

                        data_offset += round_up_power_of_two(sec_size, sec_align);
                    }
                    else {
                        return Err("no data_pages were allocated for .data section");
                    }
                }
                
                else {
                    error!("Failed to get the .data section's name after \".data.\": {:?}", sec_name);
                    return Err("Failed to get the .data section's name after \".data.\"!");
                }
            }

            else if sec_name.starts_with(BSS_PREFIX) {
                if let Some(name) = sec_name.get(BSS_PREFIX.len() ..) {
                    let demangled = demangle_symbol(name);
                    if log { trace!("Found [{}] .bss section: name {:?}, with_hash {:?}, size={:#x}", shndx, name, demangled.no_hash, sec_size); }
                    if sec_flags & (SHF_ALLOC | SHF_WRITE | SHF_EXECINSTR) != (SHF_ALLOC | SHF_WRITE) {
                        error!(".bss section [{}], name: {:?} had the wrong flags {:#X}", shndx, name, sec_flags);
                        return Err(".bss section had wrong flags!");
                    }
                    
                    // we still use DataSection to represent the .bss sections, since they have the same flags
                    if let Some(ref dp) = data_pages {
                        // here: we're ready to fill the bss section with zeroes at the proper address
                        let dest_slice: &mut [u8]  = try!(dp.as_slice_mut(data_offset, sec_size));
                        if log { 
                            let dest_addr = dest_slice as *mut [u8] as *mut u8 as VirtualAddress;
                            trace!("       dest_addr: {:#X}, data_offset: {:#X}", dest_addr, data_offset); 
                        }
                        for b in dest_slice {
                            *b = 0;
                        };

                        loaded_sections.insert(shndx, 
                            Arc::new(Mutex::new(LoadedSection::Data(DataSection{
                                abs_symbol: demangled.no_hash,
                                hash: demangled.hash,
                                mapped_pages: Arc::downgrade(dp),
                                mapped_pages_offset: data_offset,
                                size: sec_size,
                                global: global_sections.contains(&shndx),
                                parent_crate: Arc::downgrade(&new_crate),
                            })))
                        );

                        data_offset += round_up_power_of_two(sec_size, sec_align);
                    }
                    else {
                        return Err("no data_pages were allocated for .bss section");
                    }
                }
                
                else {
                    error!("Failed to get the .bss section's name after \".bss.\": {:?}", sec_name);
                    return Err("Failed to get the .bss section's name after \".bss.\"!");
                }
            }

            else {
                error!("unhandled PROGBITS/NOBITS section [{}], name: {}, sec: {:?}", shndx, sec_name, sec);
                continue;
            }

        
        }
    }  // end of handling PROGBITS sections: text, data, rodata, bss


    if log {
        debug!("=========== moving on to the relocations for module {} =========", module_name);
    }


    // Second, we need to fix up the sections we just loaded with proper relocation info
    for sec in elf_file.section_iter() {

        if let Ok(ShType::Rela) = sec.get_type() {
            // skip null section and any empty sections
            let sec_size = sec.size() as usize;
            if sec_size == 0 { continue; }

            // offset is the destination 
            use xmas_elf::sections::SectionData::Rela64;
            use xmas_elf::symbol_table::Entry;
            if log { trace!("Found Rela section name: {:?}, type: {:?}, target_sec_index: {:?}", sec.get_name(&elf_file), sec.get_type(), sec.info()); }

            // currently not using eh_frame, gcc, note, and debug sections
            if let Ok(name) = sec.get_name(&elf_file) {
                if  name.starts_with(".rela.eh_frame")   || 
                    name.starts_with(".rela.note")   ||   // ignore GNU note sections
                    name.starts_with(".rela.gcc")    ||   // ignore gcc special sections for now
                    name.starts_with(".rela.debug")       // ignore debug special sections for now
                {
                    continue;
                }
            }

            let mut target_sec_dependencies: Vec<RelocationDependency> = Vec::new();

            // the target section is where we write the relocation data to.
            // the source section is where we get the data from. 
            // There is one target section per rela section, and one source section per entry in this rela section.
            // The "info" field in the Rela section specifies which section is the target of the relocation.
            
            // check if this Rela sections has a valid target section (one that we've already loaded)
            if let Some(target_sec) = loaded_sections.get(&(sec.info() as usize)) {
                if let Ok(Rela64(rela_arr)) = sec.get_data(&elf_file) {
                    for rela_entry in rela_arr {
                        if log { 
                            trace!("      Rela64 offset: {:#X}, addend: {:#X}, symtab_index: {}, type: {:#X}", 
                                    rela_entry.get_offset(), rela_entry.get_addend(), rela_entry.get_symbol_table_index(), rela_entry.get_type());
                        }

                        // common to all relocations for this target section: calculate the relocation destination and get the source section
                        let dest_offset = target_sec.lock().mapped_pages_offset() + rela_entry.get_offset() as usize;
                        let dest_mapped_pages = target_sec.lock().mapped_pages().ok_or("couldn't get MappedPages reference for target_sec's relocation")?;
                        let source_sec_entry: &Entry = &symtab[rela_entry.get_symbol_table_index() as usize];
                        let source_sec_shndx = source_sec_entry.shndx() as usize; 
                        if log { 
                            let source_sec_header_name = source_sec_entry.get_section_header(&elf_file, rela_entry.get_symbol_table_index() as usize)
                                .and_then(|s| s.get_name(&elf_file));
                            trace!("             relevant section [{}]: {:?}", source_sec_shndx, source_sec_header_name);
                            // trace!("             Entry name {} {:?} vis {:?} bind {:?} type {:?} shndx {} value {} size {}", 
                            //     source_sec_entry.name(), source_sec_entry.get_name(&elf_file), 
                            //     source_sec_entry.get_other(), source_sec_entry.get_binding(), source_sec_entry.get_type(), 
                            //     source_sec_entry.shndx(), source_sec_entry.value(), source_sec_entry.size());
                        }


                        // We first try to get the source section from loaded_sections, which works if the section is in the crate currently being loaded.
                        let source_sec = match loaded_sections.get(&source_sec_shndx) {
                            Some(ss) => Ok(ss.clone()),

                            // If we couldn't get the section based on its shndx, it means that the source section wasn't in the crate currently being loaded.
                            // Thus, we must get the source section's name and check our list of foreign crates to see if it's there.
                            // At this point, there's no other way to search for the source section besides its name.
                            None => {
                                if let Ok(source_sec_name) = source_sec_entry.get_name(&elf_file) {
                                    const DATARELRO: &'static str = ".data.rel.ro.";
                                    let source_sec_name = if source_sec_name.starts_with(DATARELRO) {
                                        let relro_name = source_sec_name.get(DATARELRO.len() ..)
                                            .ok_or("Couldn't get name of .data.rel.ro. section")?;
                                        // warn!("relro relocation for sec {:?} -> {:?}", source_sec_name, relro_name);
                                        relro_name
                                    }
                                    else {
                                        source_sec_name
                                    };
                                    let demangled = demangle_symbol(source_sec_name);

                                    // search for the symbol's demangled name in the kernel's symbol map
                                    metadata::get_symbol_or_load(&demangled.no_hash, kernel_mmi)
                                        .upgrade()
                                        .ok_or("Couldn't get symbol for foreign relocation entry, nor load its containing crate")
                                }
                                else {
                                    let _source_sec_header = source_sec_entry
                                        .get_section_header(&elf_file, rela_entry.get_symbol_table_index() as usize)
                                        .and_then(|s| s.get_name(&elf_file));
                                    error!("Couldn't get name of source section [{}] {:?}, needed for non-local relocation entry", source_sec_shndx, _source_sec_header);
                                    Err("Couldn't get source section's name, needed for non-local relocation entry")
                                }
                            }
                        }?;

                        let source_mapped_pages = source_sec.lock().mapped_pages().ok_or("couldn't get MappedPages reference for source_sec's relocation")?;
                        let source_vaddr = source_mapped_pages.as_type::<&usize>(source_sec.lock().mapped_pages_offset())? as *const _ as usize;

                        // Write the actual relocation entries here
                        // There is a great, succint table of relocation types here
                        // https://docs.rs/goblin/0.0.13/goblin/elf/reloc/index.html
                        match rela_entry.get_type() {
                            R_X86_64_32 => {
                                let dest_ref: &mut u32 = try!(dest_mapped_pages.as_type_mut(dest_offset));
                                let dest_ptr = dest_ref as *mut _ as usize;
                                let source_val = source_vaddr.wrapping_add(rela_entry.get_addend() as usize);
                                if log { trace!("                    dest_ptr: {:#X}, source_val: {:#X} ({:?})", dest_ptr, source_val, source_sec); }
                                
                                *dest_ref = source_val as u32;
                            }
                            R_X86_64_64 => {
                                let dest_ref: &mut u64 = try!(dest_mapped_pages.as_type_mut(dest_offset));
                                let dest_ptr = dest_ref as *mut _ as usize;
                                let source_val = source_vaddr.wrapping_add(rela_entry.get_addend() as usize);
                                if log { trace!("                    dest_ptr: {:#X}, source_val: {:#X} ({:?})", dest_ptr, source_val, source_sec); }
                                
                                *dest_ref = source_val as u64;
                            }
                            R_X86_64_PC32 => {
                                let dest_ref: &mut u32 = try!(dest_mapped_pages.as_type_mut(dest_offset));
                                let dest_ptr = dest_ref as *mut _ as usize;
                                let source_val = source_vaddr.wrapping_add(rela_entry.get_addend() as usize).wrapping_sub(dest_ptr);
                                if log { trace!("                    dest_ptr: {:#X}, source_val: {:#X} ({:?})", dest_ptr, source_val, source_sec); }

                                *dest_ref = source_val as u32;
                            }
                            R_X86_64_PC64 => {
                                let dest_ref: &mut u64 = try!(dest_mapped_pages.as_type_mut(dest_offset));
                                let dest_ptr = dest_ref as *mut _ as usize;
                                let source_val = source_vaddr.wrapping_add(rela_entry.get_addend() as usize).wrapping_sub(dest_ptr);
                                if log { trace!("                    dest_ptr: {:#X}, source_val: {:#X} ({:?})", dest_ptr, source_val, source_sec); }

                                *dest_ref = source_val as u64;
                            }
                            // R_X86_64_GOTPCREL => { 
                            //     unimplemented!(); // if we stop using the large code model, we need to create a Global Offset Table
                            // }
                            _ => {
                                error!("found unsupported relocation {:?}\n  --> Are you building kernel crates with code-model=large?", rela_entry);
                                return Err("found unsupported relocation type");
                            }
                        }

                        // we only really care about tracking dependencies on other crates,
                        // since every crate has "dependencies" on itself. 
                        let target_dependency = RelocationDependency {
                            section: source_sec,
                            rel_type: rela_entry.get_type(),
                            offset: dest_offset,
                        };
                        target_sec_dependencies.push(target_dependency);
                         
                    }
                }
                else {
                    error!("Found Rela section that wasn't able to be parsed as Rela64: {:?}", sec);
                    return Err("Found Rela section that wasn't able to be parsed as Rela64");
                }

                // add dependencies to the target section
                // target_sec.lock().dependencies.append(target_sec_dependencies);
            }
            else {
                error!("Skipping Rela section {:?} for target section that wasn't loaded!", sec.get_name(&elf_file));
                continue;
            }
        }
    }

    
    // since we initially mapped the pages as writable, we need to remap them properly according to each section
    if let PageTable::Active(ref mut active_table) = kernel_mmi.page_table {
        if let Some(ref tp) = text_pages { 
            try!(active_table.remap(tp, EntryFlags::PRESENT)); // present and executable (not no_execute)
        }
        if let Some(ref rp) = rodata_pages { 
            try!(active_table.remap(rp, EntryFlags::PRESENT | EntryFlags::NO_EXECUTE)); // present (just readable)
        }
        if let Some(ref dp) = data_pages { 
            try!(active_table.remap(dp, EntryFlags::PRESENT | EntryFlags::WRITABLE | EntryFlags::NO_EXECUTE)); // read/write
        }
    }
    else {
        return Err("couldn't get kernel's active page table");
    }
    
    // extract just the section refs from the loaded_section map
    let (_keys, values): (Vec<usize>, Vec<StrongSectionRef>) = loaded_sections.into_iter().unzip();

    {
        let mut new_crate_locked = new_crate.write();
        new_crate_locked.sections     = values;
        new_crate_locked.text_pages   = text_pages;
        new_crate_locked.rodata_pages = rodata_pages;
        new_crate_locked.data_pages   = data_pages;
    }
    Ok(new_crate)
}



/// Decides which parsing technique to use, either the symbol file or the actual binary file.
/// `parse_nano_core_binary()` is VERY SLOW in debug mode for large binaries, so we use the more efficient symbol file parser instead.
/// Note that this must match the setup of the kernel/Makefile as well as the cfg/grub.cfg entry
const PARSE_NANO_CORE_SYMBOL_FILE: bool = true;


// Parses the nano_core module that represents the already loaded (and currently running) nano_core code.
// Basically, just searches for global (public) symbols, which are added to the system map and the crate metadata.
pub fn parse_nano_core(kernel_mmi: &mut MemoryManagementInfo, 
    text_pages: Arc<MappedPages>, 
    rodata_pages: Arc<MappedPages>, 
    data_pages: Arc<MappedPages>, 
    log: bool) 
    -> Result<usize, &'static str> 
{
    debug!("parse_nano_core: trying to load and parse the nano_core file");
    let module = try!(get_module("__k_nano_core").ok_or("Couldn't find module called __k_nano_core"));
    use kernel_config::memory::address_is_page_aligned;
    if !address_is_page_aligned(module.start_address()) {
        error!("module {} is not page aligned!", module.name());
        return Err("nano_core module was not page aligned");
    } 

    // below, we map the nano_core module file just so we can parse it. We don't need to actually load it since we're already running it.

    let &mut MemoryManagementInfo { 
        page_table: ref mut kernel_page_table, 
        ..  // don't need to access the kernel's vmas or stack allocator, we already allocated a kstack above
    } = kernel_mmi;

    match kernel_page_table {
        &mut PageTable::Active(ref mut active_table) => {
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

            let new_syms = metadata::add_crate(new_crate, log);
            info!("parsed nano_core crate, {} new symbols.", new_syms);
            Ok(new_syms)

            // temp_module_mapping is automatically unmapped when it falls out of scope here (frame allocator must not be locked)
        }
        _ => {
            error!("parse_nano_core(): error getting kernel's active page table to map module.");
            Err("couldn't get kernel's active page table")
        }
    }
}



/// Parses the nano_core symbol file that represents the already loaded (and currently running) nano_core code.
/// Basically, just searches for global (public) symbols, which are added to the system map and the crate metadata.
/// 
/// Drops the given `mapped_pages` that hold the nano_core module file itself.
pub fn parse_nano_core_symbol_file(mapped_pages: MappedPages, 
    text_pages: Arc<MappedPages>, 
    rodata_pages: Arc<MappedPages>, 
    data_pages: Arc<MappedPages>, 
    size: usize) 
    -> Result<Arc<RwLock<LoadedCrate>>, &'static str> 
{
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
                sections.push(Arc::new(Mutex::new(
                    LoadedSection::Text(TextSection{
                        abs_symbol: no_hash,
                        hash: hash,
                        mapped_pages: Arc::downgrade(&text_pages),
                        mapped_pages_offset: text_pages.offset_of(sec_vaddr).ok_or("nano_core text section wasn't covered by its mapped pages!")?,
                        size: sec_size,
                        global: true,
                        parent_crate: Arc::downgrade(&new_crate),
                    }))
                ));
            }
            else if sec_ndx == rodata_shndx {
                sections.push(Arc::new(Mutex::new(
                    LoadedSection::Rodata(RodataSection{
                        abs_symbol: no_hash,
                        hash: hash,
                        mapped_pages: Arc::downgrade(&rodata_pages),
                        mapped_pages_offset: rodata_pages.offset_of(sec_vaddr).ok_or("nano_core rodata section wasn't covered by its mapped pages!")?,
                        size: sec_size,
                        global: true,
                        parent_crate: Arc::downgrade(&new_crate),
                    }))
                ));
            }
            else if (sec_ndx == data_shndx) || (sec_ndx == bss_shndx) {
                sections.push(Arc::new(Mutex::new(
                    LoadedSection::Data(DataSection{
                        abs_symbol: no_hash,
                        hash: hash,
                        mapped_pages: Arc::downgrade(&data_pages),
                        mapped_pages_offset: data_pages.offset_of(sec_vaddr).ok_or("nano_core data/bss section wasn't covered by its mapped pages!")?,
                        size: sec_size,
                        global: true,
                        parent_crate: Arc::downgrade(&new_crate),
                    }))
                ));
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
fn parse_nano_core_binary(mapped_pages: MappedPages, 
    text_pages: Arc<MappedPages>, 
    rodata_pages: Arc<MappedPages>, 
    data_pages: Arc<MappedPages>, 
    size_in_bytes: usize) 
    -> Result<Arc<RwLock<LoadedCrate>>, &'static str> 
{
    let crate_name = String::from("nano_core");
    debug!("Parsing {} binary: size {:#x}({}), MappedPages: {:?}, text_pages: {:?}, rodata_pages: {:?}, data_pages: {:?}", 
            crate_name, size_in_bytes, size_in_bytes, mapped_pages, text_pages, rodata_pages, data_pages);

    let byte_slice: &[u8] = mapped_pages.as_slice(0, size_in_bytes)?;
    // debug!("BYTE SLICE: {:?}", byte_slice);
    let elf_file = ElfFile::new(byte_slice)?; // returns Err(&str) if ELF parse fails

    // For us to properly load the ELF file, it must NOT have been stripped,
    // meaning that it must still have its symbol table section. Otherwise, relocations will not work.
    use xmas_elf::sections::SectionData::SymbolTable64;
    let sssec = find_first_section_by_type(&elf_file, ShType::SymTab);
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

                            let demangled = demangle_symbol(name);
                            // debug!("parse_nano_core_binary(): name: {}, demangled: {}, vaddr: {:#X}, size: {:#X}", name, demangled.no_hash, sec_vaddr, sec_size);

                            let new_section = {
                                if entry.shndx() as usize == text_shndx {
                                    Some(LoadedSection::Text(TextSection{
                                        abs_symbol: demangled.no_hash,
                                        hash: demangled.hash,
                                        mapped_pages: Arc::downgrade(&text_pages),
                                        mapped_pages_offset: try!(text_pages.offset_of(sec_vaddr).ok_or("nano_core text section wasn't covered by its mapped pages!")),
                                        size: sec_size,
                                        global: true,
                                        parent_crate: Arc::downgrade(&new_crate),
                                    }))
                                }
                                else if entry.shndx() as usize == rodata_shndx {
                                    Some(LoadedSection::Rodata(RodataSection{
                                        abs_symbol: demangled.no_hash,
                                        hash: demangled.hash,
                                        mapped_pages: Arc::downgrade(&rodata_pages),
                                        mapped_pages_offset: try!(rodata_pages.offset_of(sec_vaddr).ok_or("nano_core rodata section wasn't covered by its mapped pages!")),
                                        size: sec_size,
                                        global: true,
                                        parent_crate: Arc::downgrade(&new_crate),
                                    }))
                                }
                                else if (entry.shndx() as usize == data_shndx) || (entry.shndx() as usize == bss_shndx) {
                                    Some(LoadedSection::Data(DataSection{
                                        abs_symbol: demangled.no_hash,
                                        hash: demangled.hash,
                                        mapped_pages: Arc::downgrade(&data_pages),
                                        mapped_pages_offset: try!(data_pages.offset_of(sec_vaddr).ok_or("nano_core data/bss section wasn't covered by its mapped pages!")),
                                        size: sec_size,
                                        global: true,
                                        parent_crate: Arc::downgrade(&new_crate),
                                    }))
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


/// Parses a section index out of a string like "[7]"
fn parse_section_ndx<'a>(s: &str) -> Option<usize> {
    let open  = s.find("[");
    let close = s.find("]");
    open.and_then(|start| close.and_then(|end| s.get((start + 1) .. end)))
        .and_then(|t| t.trim().parse::<usize>().ok())
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
