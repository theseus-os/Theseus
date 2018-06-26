#![no_std]
#![feature(alloc)]
#![feature(rustc_private)]
// #![feature(nll)]

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
extern crate owning_ref;


use core::ops::DerefMut;
use alloc::{Vec, BTreeMap, BTreeSet, String};
use alloc::string::ToString;
use alloc::btree_map::Entry;
use alloc::arc::{Arc, Weak};
use spin::{Mutex, RwLock};

use xmas_elf::ElfFile;
use xmas_elf::sections::{SectionData, ShType};
use xmas_elf::sections::{SHF_WRITE, SHF_ALLOC, SHF_EXECINSTR};
use goblin::elf::reloc::*;

use util::round_up_power_of_two;
use memory::{FRAME_ALLOCATOR, get_module, MemoryManagementInfo, ModuleArea, Frame, PageTable, VirtualAddress, MappedPages, EntryFlags, allocate_pages_by_bytes};

use metadata::{StrongCrateRef, WeakSectionRef};


pub mod demangle;
pub mod elf_executable;
pub mod parse_nano_core;
pub mod metadata;
pub mod dependency;

use self::metadata::*;
use self::dependency::*;

use demangle::demangle_symbol;


// Can also try this crate: https://crates.io/crates/goblin
// ELF RESOURCE: http://www.cirosantilli.com/elf-hello-world


lazy_static! {
    /// The initial `CrateNamespace` that all crates are added to by default,
    /// unless otherwise specified for crate swapping purposes.
    static ref DEFAULT_CRATE_NAMESPACE: CrateNamespace = CrateNamespace::new();
}

pub fn get_default_namespace() -> &'static CrateNamespace {
    &DEFAULT_CRATE_NAMESPACE
}



/// A "symbol map" from a fully-qualified demangled symbol String  
/// to weak reference to a `LoadedSection`.
/// This is used for relocations, and for looking up function names.
pub type SymbolMap = BTreeMap<String, WeakSectionRef>;


/// This struct represents a namespace of crates and their "global" (publicly-visible) symbols.
/// A crate namespace struct is basically a container around many crates 
/// that have all been loaded and linked against each other, 
/// completely separate and in isolation from any other crate namespace 
/// (although a given crate may be present in multiple namespaces). 
pub struct CrateNamespace {
    /// The list of all the crates in this namespace,
    /// stored as a map in which the crate's String name
    /// is the key that maps to the value, a strong reference to a crate.
    /// It is a strong reference because a crate must not be removed
    /// as long as it is part of any namespace,
    /// and a single crate can be part of multiple namespaces at once.
    /// For example, the "core" (Rust core library) crate is essentially
    /// part of every single namespace, simply because most other crates rely upon it. 
    crate_tree: Mutex<BTreeMap<String, StrongCrateRef>>,

    /// The "system map" of all global (publicly-visible) symbols
    /// that are present in all of the crates in this `CrateNamespace`.
    /// Maps a fully-qualified symbol name string to a corresponding `LoadedSection`,
    /// which is guaranteed to be part of one of the crates in this `CrateNamespace`.  
    /// Symbols declared as "no_mangle" will appear  root namespace with no crate prefixex, as expected.
    symbol_map: Mutex<SymbolMap>,
}

impl CrateNamespace {
    /// Creates a new `CrateNamespace` that is completely empty. 
    pub fn new() -> CrateNamespace {
        CrateNamespace {
            crate_tree: Mutex::new(BTreeMap::new()),
            symbol_map: Mutex::new(SymbolMap::new()),
        }
    } 


    /// Returns a list of all of the crate names currently loaded into this `CrateNamespace`.
    pub fn crate_names(&self) -> Vec<String> {
        self.crate_tree.lock().keys().cloned().collect()
    }


    /// Returns a strong reference to the `LoadedCrate` in this namespace 
    /// that matches the given `crate_name`, if it has been loaded into this namespace.
    pub fn get_crate(&self, crate_name: &str) -> Option<StrongCrateRef> {
        self.crate_tree.lock().get(crate_name).cloned()
    }


    /// Loads the specified application crate into memory, allowing it to be invoked.  
    /// Unlike [`load_kernel_crate`](#method.load_kernel_crate), this does not add the newly-loaded
    /// application crate to this namespace, nor does it add the new crate's symbols to the 
    /// Instead, it returns a Result containing the new crate itself.
    pub fn load_application_crate(
        &self, 
        crate_module: &ModuleArea, 
        kernel_mmi: &mut MemoryManagementInfo, 
        verbose_log: bool) 
        -> Result<Arc<RwLock<LoadedCrate>>, &'static str> 
    {
        if !CrateType::is_application(crate_module.name()) {
            error!("load_application_crate() cannot be used for crate \"{}\", only for application crate modules starting with \"{}\"",
                crate_module.name(), CrateType::Application.prefix());
            return Err("load_application_crate() can only be used for application crate modules");
        }
        
        debug!("load_application_crate: trying to load \"{}\" application module", crate_module.name());
        let temp_module_mapping = map_crate_module(crate_module, kernel_mmi)?;

        let plc = self.load_crate_sections(&temp_module_mapping, crate_module.size(), crate_module.name(), kernel_mmi, verbose_log)?;
        // no backup namespace when loading applications, they must be able to find all symbols in only this namespace (&self)
        let new_crate = self.perform_relocations(&plc.elf_file, plc.new_crate, plc.loaded_sections, None, kernel_mmi, verbose_log)?;
        { 
            let new_crate_locked = new_crate.read(); 
            info!("loaded new application crate module: {}, num sections: {}", new_crate_locked.crate_name, new_crate_locked.sections.len());
        }
        Ok(new_crate)

        // plc.temp_module_mapping is automatically unmapped when it falls out of scope here (frame allocator must not be locked)
    }


    /// Loads the specified kernel crate into memory, allowing it to be invoked.  
    /// Returns a Result containing the number of symbols that were added to the symbol map
    /// as a result of loading this crate.
    /// # Arguments
    /// * `crate_module`: the crate that should be loaded into this `CrateNamespace`.
    /// * `backup_namespace`: the `CrateNamespace` that should be searched for missing symbols 
    ///   (for relocations) if a symbol cannot be found in this `CrateNamespace`. 
    ///   For example, the default namespace could be used by passing in `Some(get_default_namespace())`.
    ///   If `backup_namespace` is `None`, then no other namespace will be searched, 
    ///   and any missing symbols will return an `Err`. 
    /// * `kernel_mmi`: a mutable reference to the kernel's `MemoryManagementInfo`.
    /// * `verbose_log`: a boolean value whether to enable verbose_log logging of crate loading actions.
    pub fn load_kernel_crate(
        &self,
        crate_module: &ModuleArea, 
        backup_namespace: Option<&CrateNamespace>, 
        kernel_mmi: &mut MemoryManagementInfo, 
        verbose_log: bool
    ) -> Result<usize, &'static str> {

        debug!("load_kernel_crate: trying to load \"{}\" kernel module", crate_module.name());
        let temp_module_mapping = map_crate_module(crate_module, kernel_mmi)?;
        let plc = self.load_crate_sections(&temp_module_mapping, crate_module.size(), crate_module.name(), kernel_mmi, verbose_log)?;
        let new_crate = self.perform_relocations(&plc.elf_file, plc.new_crate, plc.loaded_sections, backup_namespace, kernel_mmi, verbose_log)?;
        let crate_name = new_crate.read().crate_name.clone();
        let new_syms = self.add_symbols(new_crate.read().sections.iter(), &crate_name, verbose_log);
        self.crate_tree.lock().insert(crate_name, new_crate);
        info!("loaded new crate module: {}, {} new symbols.", crate_module.name(), new_syms);
        Ok(new_syms)
        
        // plc.temp_module_mapping is automatically unmapped when it falls out of scope here (frame allocator must not be locked)
    }

        
    /// Swaps in new modules to replace existing crates this in `CrateNamespace`.
    /// This function accepts several modules in order to allow swapping multiple crates all at once 
    /// in a single "atomic" unit, which prevents weird linking/relocation errors, 
    /// such as a new crate linking against an old crate that already exists in this namespace
    /// instead of linking against the new one that we want to replace that old crate with. 
    /// 
    /// In general, the strategy for replacing and old module `C` with a new module `C'` consists of three simple steps:
    /// 1) Load the new replacement module `C'`.
    /// 2) Set up new relocation entries that redirect all module's dependencies on the old module `C` to the new module `C'`.
    /// 3) Remove module `C` and clean it up, e.g., removing its entries from the symbol map.
    /// 
    /// This `CrateNamespace` (self) is used as the backup namespace for resolving unknown symbols.
    /// 
    /// Upon a successful return, this namespace (self) will have the new crates in place of the old ones,
    /// and the old crates will be completely removed from this namespace. 
    pub fn swap_crates(
        &self,
        swap_pairs: Vec<(StrongCrateRef, &ModuleArea, Option<String>)>,
        kernel_mmi: &mut MemoryManagementInfo,
        verbose_log: bool,
    ) -> Result<(), &'static str> {
        // create a new CrateNamespace and load all of the new crate modules into it
        let swap_pairs = swap_pairs.into_iter();
        let module_iter = swap_pairs.clone().map(|pair| pair.1);
        let new_namespace = load_crates_in_new_namespace(module_iter, self, kernel_mmi, verbose_log)?;

        // for (crate_name, crate_ref) in new_namespace.crate_tree.lock().iter() {
        //     debug!("====================== Loaded new crate \"{}\"  ===========================", crate_name);
        //     let krate = crate_ref.read();
        //     krate.text_pages.as_ref().map(|tp|   debug!("    text_pages  : {:#X} ({} pages)", tp.start_address(), tp.size_in_pages()));
        //     krate.rodata_pages.as_ref().map(|rp| debug!("    rodata_pages: {:#X} ({} pages)", rp.start_address(), rp.size_in_pages()));
        //     krate.data_pages.as_ref().map(|dp|   debug!("    data_pages  : {:#X} ({} pages)\n\n", dp.start_address(), dp.size_in_pages()));
        // }

        // Now that we have loaded all of the new modules into the new namepsace in isolation,
        // we simply need to remove all of the old crates
        // and fix up all of the relocations `WeakDependents` for each of the existing sections
        // that depend on the old crate that we're replacing here,
        // such that they refer to the new_module instead of the old_crate.
        for (old_crate_ref, new_module, override_new_crate_name) in swap_pairs {
            let (_new_crate_type, new_crate_name) = CrateType::from_module_name(new_module.name())?;
            let new_crate_ref = new_namespace.get_crate(new_crate_name)
                .ok_or_else(|| "Couldn't get new crate from new namespace")?;

            // if requested, override the new crate's name and section prefixes and symbol prefixes
            let new_crate_name = if let Some(override_name) = override_new_crate_name {
                debug!("Overriding new crate name \"{}\" with \"{}\"", new_crate_name, override_name);
                let mut new_crate = new_crate_ref.write();
                for new_sec_ref in &new_crate.sections {
                    let mut new_sec = new_sec_ref.lock();
                    // debug!("  Looking at {:?} section \"{}#{:?}\" (global: {})", new_sec.typ, new_sec.name, new_sec.hash, new_sec.global);
                    if let Some(new_name) = replace_parent_crate_name(&new_sec.name, &new_crate.crate_name, &override_name) {
                        debug!("    Overriding new section name: \"{}\" --> \"{}\"", new_sec.name, new_name);
                        if new_sec.global {
                            new_namespace.replace_symbol_key(&new_sec.name, &new_name)?;
                        }
                        new_sec.name = new_name;
                    }
                }

                new_crate.crate_name = override_name.clone();
                override_name
            } else {
                new_crate_name.to_string()
            };

            let old_crate = old_crate_ref.read();


            debug!("====================== Replacing old_crate \"{}\"  ===========================", old_crate.crate_name);
            old_crate.text_pages.as_ref().map(|tp|   debug!("  text_pages  : {:#X} ({} pages)", tp.start_address(), tp.size_in_pages()));
            old_crate.rodata_pages.as_ref().map(|rp| debug!("  rodata_pages: {:#X} ({} pages)", rp.start_address(), rp.size_in_pages()));
            old_crate.data_pages.as_ref().map(|dp|   debug!("  data_pages  : {:#X} ({} pages)\n\n", dp.start_address(), dp.size_in_pages()));

            
            // debug!("  Dependent crates:");
            // dump_dependent_crates(&*old_crate, String::from("    "));
            // for sec_ref in &old_crate.sections {
            //     let sec = sec_ref.lock();
            //     if false {
            //         if !sec.sections_i_depend_on.is_empty() {
            //             debug!("  Section \"{}\": sections i depend on (strong dependencies):", sec.name);
            //             for strong_dep in &sec.sections_i_depend_on {
            //                 debug!("        {}", strong_dep.section.lock().name);
            //             }
            //         }
            //     }
            //     if true {
            //         if !sec.sections_dependent_on_me.is_empty() {
            //             let prefix = String::from("  ");
            //             dump_weak_dependents(&*sec, prefix.clone())
            //         }
            //     }
            // }

            debug!("Rewriting relocation dependencies for {} -> {}", old_crate.crate_name, new_crate_name);

            // We need to find all of the weak dependents (sections that depend on sections in the old crate that we're removing)
            // and replace them by rewriting their relocation entries to point to that section in the new_crate.
            // We also use this loop to remove all of the old_crate's symbols from this namespace's symbol map.
            for old_sec_ref in &old_crate.sections {

                let mut old_sec = old_sec_ref.lock();
                for weak_dep in &old_sec.sections_dependent_on_me {
                    let target_sec = weak_dep.section.upgrade().ok_or_else(|| "couldn't upgrade WeakDependent.section")?;

                    // Use the new namespace to find the new source_sec that old target_sec should point to.
                    // The new source_sec must have the same name as the old one (old_sec here),
                    // otherwise it wouldn't be a valid swap -- the target_sec's parent crate should have also been swapped.
                    // The new namespace should already have that symbol available (i.e., we shouldn't have to load it on demand);
                    // if not, the swapping action was never going to work and we shouldn't go through with it
                    let source_sec = new_namespace.get_symbol(&old_sec.name).upgrade().ok_or_else(|| 
                        "couldn't find replacement source section in new namespace that the target section needs to point to, perhaps the target section must also be swapped?"
                    )?;

                    let res = write_relocation(weak_dep.relocation, &target_sec, None, &source_sec, kernel_mmi, true || verbose_log)?;
                    let (new_strong_dep, new_weak_dep) = res.ok_or_else(|| "logic error: write_relocation() was successful, \
                        but didn't return any new dependencies, meaning that the source_sec and target_sec \
                        were in the same parent crate (which should never happen when fixing up relocations for weak dependencies"
                    )?;

                    // Tell the old source_sec that it no longer has the existing target_sec as a dependent
                        // this is performed all at once at the end of this loop: we clear the whole old_sec.sections_dependent_on_me
                    // Tell the new source_sec that the existing target_sec depends on it.
                    source_sec.lock().sections_dependent_on_me.push(new_weak_dep);

                    // Tell the existing target_sec that it no longer depends on the old source section (old_sec_ref),
                    // and that it now depends on the new source_sec.
                    {
                        let mut ts = target_sec.lock();
                        let index = ts.find_strong_dependency(&old_sec_ref)
                            .ok_or("Couldn't find/remove the target_sec's StrongDependency on the old crate section")?;
                        ts.sections_i_depend_on.remove(index);
                        ts.sections_i_depend_on.push(new_strong_dep);
                    }
                }

                // As described in the above loop, we must tell the old section (the one we're replacing)
                // that no other sections rely on it. 
                old_sec.sections_dependent_on_me.clear();
            
                // Currently we are just copying over the old_sec into the new source_sec,
                // if they represent a static variable (state spill that would otherwise result in a loss of data).
                // Currently, static variables (states) are only .bss sections
                if old_sec.typ == SectionType::Bss {
                    let mut new_crate = new_crate_ref.write();
                    let new_dest_sec = new_crate.find_section(|sec| sec.name == old_sec.name).ok_or_else(|| 
                        "couldn't find destination section in new crate for copying old_sec's data into (BSS state transfer)"
                    )?;
                    old_sec.copy_section_data_to(&old_crate, &mut new_dest_sec.lock(), &mut new_crate)?;
                }
            }
            
            // remove the old crate from this namespace, and remove its sections' symbols too
            self.remove_symbols(old_crate.sections.iter(), true);
            if self.crate_tree.lock().remove(&old_crate.crate_name).is_some() {
                info!("  Removed old crate {}", old_crate.crate_name);
            }

            // add the new crate and its sections' symbols to this namespace
            self.add_symbols(new_crate_ref.read().sections.iter(), &new_crate_name, verbose_log);
            self.crate_tree.lock().insert(new_crate_name, new_crate_ref);

        }


        Ok(())

        // here, "new_namespace is dropped, but its crates have already been added 
    }



    /// The primary internal routine for parsing and loading all of the sections.
    /// This does not perform any relocations or linking, so the crate is not yet ready to use after this function.
    /// However, it does add all of the newly-loaded crate sections to the symbol map (yes, even before relocation/linking),
    /// since we can use them to resolve missing symbols for relocations.
    /// Parses each section in the given `ElfFile` and copies the object file contents to each section.
    /// Returns a tuple of the new `LoadedCrate`, the list of newly `LoadedSection`s, and the crate's ELF file.
    /// The list of sections is actually a map from its section index (shndx) to the `LoadedSection` itself,
    /// which is kept separate and has not yet been added to the new `LoadedCrate` beause it needs to be used for relocations.
    fn load_crate_sections<'e>(
        &self,
        mapped_pages: &'e MappedPages, 
        size_in_bytes: usize, 
        module_name: &String, 
        kernel_mmi: &mut MemoryManagementInfo,
        verbose_log: bool
    ) -> Result<PartiallyLoadedCrate<'e>, &'static str> {
        
        let (_crate_type, crate_name) = CrateType::from_module_name(module_name)?;
        let byte_slice: &[u8] = mapped_pages.as_slice(0, size_in_bytes)?;
        let elf_file = ElfFile::new(byte_slice)?; // returns Err(&str) if ELF parse fails

        // check that elf_file is a relocatable type 
        use xmas_elf::header::Type;
        let typ = elf_file.header.pt2.type_().as_type();
        if typ != Type::Relocatable {
            error!("load_crate_sections(): crate \"{}\" was a {:?} Elf File, must be Relocatable!", crate_name, typ);
            return Err("not a relocatable elf file");
        }

        debug!("Parsing Elf kernel crate: {:?}, size {:#x}({})", module_name, size_in_bytes, size_in_bytes);

        // allocate enough space to load the sections
        let mut section_pages = allocate_section_pages(&elf_file, kernel_mmi)?;

        // iterate through the symbol table so we can find which sections are global (publicly visible)
        // we keep track of them here in a list
        let global_sections: BTreeSet<usize> = {
            // For us to properly load the ELF file, it must NOT have been stripped,
            // meaning that it must still have its symbol table section. Otherwise, relocations will not work.
            let symtab = find_symbol_table(&elf_file)?;

            let mut globals: BTreeSet<usize> = BTreeSet::new();
            use xmas_elf::symbol_table::Entry;
            for entry in symtab.iter() {
                if let Ok(typ) = entry.get_type() {
                    if typ == xmas_elf::symbol_table::Type::Func || typ == xmas_elf::symbol_table::Type::Object {
                        use xmas_elf::symbol_table::Visibility;
                        if let Visibility::Default = entry.get_other() {
                            if entry.get_binding() == Ok(xmas_elf::symbol_table::Binding::Global) {
                                globals.insert(entry.shndx() as usize);
                            }
                        }
                    }
                }
            }
            globals 
        };

        // this maps section header index (shndx) to LoadedSection
        let mut loaded_sections: BTreeMap<usize, StrongSectionRef> = BTreeMap::new(); 

        // We create the new crate here so we can obtain references to it later.
        // The latter fields will be filled in later.
        let new_crate = Arc::new(RwLock::new(
            LoadedCrate {
                crate_name:   String::from(crate_name), 
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
                        error!("load_crate_sections: couldn't get section name for section [{}]: {:?}\n    error: {}", shndx, sec, _e);
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
                            error!("load_crate_sections(): Couldn't get next section for zero-sized section {}", shndx);
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
                        if verbose_log { trace!("Found [{}] .text section: name {:?}, with_hash {:?}, size={:#x}", shndx, name, demangled.no_hash, sec_size); }
                        if sec_flags & (SHF_ALLOC | SHF_WRITE | SHF_EXECINSTR) != (SHF_ALLOC | SHF_EXECINSTR) {
                            error!(".text section [{}], name: {:?} had the wrong flags {:#X}", shndx, name, sec_flags);
                            return Err(".text section had wrong flags!");
                        }

                        if let Some(ref mut tp) = section_pages.text_pages {
                            // here: we're ready to copy the text section to the proper address
                            let dest_slice: &mut [u8]  = try!(tp.as_slice_mut(text_offset, sec_size));
                            if verbose_log { 
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
                                    error!("load_crate_sections(): Couldn't get section data for .text section [{}] {}: {:?}", shndx, sec_name, sec.get_data(&elf_file));
                                    return Err("couldn't get section data in .text section");
                                }
                            }
                
                            loaded_sections.insert(shndx, 
                                Arc::new(Mutex::new(LoadedSection::new(
                                    SectionType::Text,
                                    demangled.no_hash,
                                    demangled.hash,
                                    text_offset,
                                    sec_size,
                                    global_sections.contains(&shndx),
                                    Arc::downgrade(&new_crate),
                                )))
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
                        if verbose_log { trace!("Found [{}] .rodata section: name {:?}, demangled {:?}, size={:#x}", shndx, name, demangled.no_hash, sec_size); }
                        if sec_flags & (SHF_ALLOC | SHF_WRITE | SHF_EXECINSTR) != (SHF_ALLOC) {
                            error!(".rodata section [{}], name: {:?} had the wrong flags {:#X}", shndx, name, sec_flags);
                            return Err(".rodata section had wrong flags!");
                        }

                        if let Some(ref mut rp) = section_pages.rodata_pages {
                            // here: we're ready to copy the rodata section to the proper address
                            let dest_slice: &mut [u8]  = try!(rp.as_slice_mut(rodata_offset, sec_size));
                            if verbose_log { 
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
                                    error!("load_crate_sections(): Couldn't get section data for .rodata section [{}] {}: {:?}", shndx, sec_name, sec.get_data(&elf_file));
                                    return Err("couldn't get section data in .rodata section");
                                }
                            }

                            loaded_sections.insert(shndx, 
                                Arc::new(Mutex::new(LoadedSection::new(
                                    SectionType::Rodata,
                                    demangled.no_hash,
                                    demangled.hash,
                                    rodata_offset,
                                    sec_size,
                                    global_sections.contains(&shndx),
                                    Arc::downgrade(&new_crate),
                                )))
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
                        if verbose_log { trace!("Found [{}] .data section: name {:?}, with_hash {:?}, size={:#x}", shndx, name, demangled.no_hash, sec_size); }
                        if sec_flags & (SHF_ALLOC | SHF_WRITE | SHF_EXECINSTR) != (SHF_ALLOC | SHF_WRITE) {
                            error!(".data section [{}], name: {:?} had the wrong flags {:#X}", shndx, name, sec_flags);
                            return Err(".data section had wrong flags!");
                        }
                        
                        if let Some(ref mut dp) = section_pages.data_pages {
                            // here: we're ready to copy the data/bss section to the proper address
                            let dest_slice: &mut [u8]  = try!(dp.as_slice_mut(data_offset, sec_size));
                            if verbose_log { 
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
                                    error!("load_crate_sections(): Couldn't get section data for .data section [{}] {}: {:?}", shndx, sec_name, sec.get_data(&elf_file));
                                    return Err("couldn't get section data in .data section");
                                }
                            }

                            loaded_sections.insert(shndx, 
                                Arc::new(Mutex::new(LoadedSection::new(
                                    SectionType::Data,
                                    demangled.no_hash,
                                    demangled.hash,
                                    data_offset,
                                    sec_size,
                                    global_sections.contains(&shndx),
                                    Arc::downgrade(&new_crate),
                                )))
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
                        if verbose_log { trace!("Found [{}] .bss section: name {:?}, with_hash {:?}, size={:#x}", shndx, name, demangled.no_hash, sec_size); }
                        if sec_flags & (SHF_ALLOC | SHF_WRITE | SHF_EXECINSTR) != (SHF_ALLOC | SHF_WRITE) {
                            error!(".bss section [{}], name: {:?} had the wrong flags {:#X}", shndx, name, sec_flags);
                            return Err(".bss section had wrong flags!");
                        }
                        
                        // we still use DataSection to represent the .bss sections, since they have the same flags
                        if let Some(ref mut dp) = section_pages.data_pages {
                            // here: we're ready to fill the bss section with zeroes at the proper address
                            let dest_slice: &mut [u8]  = try!(dp.as_slice_mut(data_offset, sec_size));
                            if verbose_log { 
                                let dest_addr = dest_slice as *mut [u8] as *mut u8 as VirtualAddress;
                                trace!("       dest_addr: {:#X}, data_offset: {:#X}", dest_addr, data_offset); 
                            }
                            for b in dest_slice {
                                *b = 0;
                            };

                            loaded_sections.insert(shndx, 
                                Arc::new(Mutex::new(LoadedSection::new(
                                    SectionType::Bss,
                                    demangled.no_hash,
                                    demangled.hash,
                                    data_offset,
                                    sec_size,
                                    global_sections.contains(&shndx),
                                    Arc::downgrade(&new_crate),
                                )))
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
        }

        // Now that we have loaded the section content above into the mapped pages, 
        // we can place them into the ownership of the `LoadedCrate`. 
        {
            let mut new_crate_locked = new_crate.write();
            new_crate_locked.text_pages   = section_pages.text_pages;
            new_crate_locked.rodata_pages = section_pages.rodata_pages;
            new_crate_locked.data_pages   = section_pages.data_pages;
        }

        Ok(PartiallyLoadedCrate {
            new_crate: new_crate,
            loaded_sections: loaded_sections,
            elf_file: elf_file,
        })
    }

        
    /// The second stage of parsing and loading a new kernel crate, 
    /// filling in the missing relocation information in the already-loaded sections. 
    /// It also remaps the `new_crate`'s MappedPages according to each of their section permissions.
    fn perform_relocations(
        &self,
        elf_file: &ElfFile,
        new_crate: StrongCrateRef,
        loaded_sections: BTreeMap<usize, StrongSectionRef>,
        backup_namespace: Option<&CrateNamespace>,
        kernel_mmi: &mut MemoryManagementInfo,
        verbose_log: bool
    ) -> Result<StrongCrateRef, &'static str> {

        if verbose_log { debug!("=========== moving on to the relocations for crate {} =========", new_crate.read().crate_name); }
        let symtab = find_symbol_table(&elf_file)?;

        // Fix up the sections that were just loaded, using proper relocation info.
        // Iterate over every non-zero relocation section in the file
        for sec in elf_file.section_iter().filter(|sec| sec.get_type() == Ok(ShType::Rela) && sec.size() != 0) {
            use xmas_elf::sections::SectionData::Rela64;
            use xmas_elf::symbol_table::Entry;
            if verbose_log { 
                trace!("Found Rela section name: {:?}, type: {:?}, target_sec_index: {:?}", 
                sec.get_name(&elf_file), sec.get_type(), sec.info()); 
            }

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

            let rela_array = match sec.get_data(&elf_file) {
                Ok(Rela64(rela_arr)) => rela_arr,
                _ => {
                    error!("Found Rela section that wasn't able to be parsed as Rela64: {:?}", sec);
                    return Err("Found Rela section that wasn't able to be parsed as Rela64");
                } 
            };

            // The target section is where we write the relocation data to.
            // The source section is where we get the data from. 
            // There is one target section per rela section (`rela_array`), and one source section per rela_entry in this rela section.
            // The "info" field in the Rela section specifies which section is the target of the relocation.
                
            // Get the target section (that we already loaded) for this rela_array Rela section.
            let target_sec = loaded_sections.get(&(sec.info() as usize)).ok_or_else(|| {
                error!("ELF file error: target section was not loaded for Rela section {:?}!", sec.get_name(&elf_file));
                "target section was not loaded for Rela section"
            })?; 
            let mut target_sec_dependencies: Vec<StrongDependency> = Vec::new();

            let target_sec_parent_crate_ref = target_sec.lock().parent_crate.upgrade().ok_or("Couldn't get target_sec's parent_crate")?;

            // iterate through each relocation entry in the relocation array for the target_sec
            for rela_entry in rela_array {
                if verbose_log { 
                    trace!("      Rela64 offset: {:#X}, addend: {:#X}, symtab_index: {}, type: {:#X}", 
                        rela_entry.get_offset(), rela_entry.get_addend(), rela_entry.get_symbol_table_index(), rela_entry.get_type());
                }

                let source_sec_entry: &Entry = &symtab[rela_entry.get_symbol_table_index() as usize];
                let source_sec_shndx = source_sec_entry.shndx() as usize; 
                if verbose_log { 
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
                                source_sec_name.get(DATARELRO.len() ..)
                                    .ok_or("Couldn't get name of .data.rel.ro. section")?
                            }
                            else {
                                source_sec_name
                            };
                            let demangled = demangle_symbol(source_sec_name);

                            // search for the symbol's demangled name in the kernel's symbol map
                            self.get_symbol_or_load(&demangled.no_hash, backup_namespace, kernel_mmi, verbose_log)
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

                let deps = write_relocation(
                    RelocationEntry::from_elf_relocation(rela_entry),
                    target_sec,
                    Some(target_sec_parent_crate_ref.clone()),
                    &source_sec,
                    kernel_mmi,
                    verbose_log
                )?;
                if let Some((strong_dependency, weak_dependent)) = deps {
                    target_sec_dependencies.push(strong_dependency);
                    source_sec.lock().sections_dependent_on_me.push(weak_dependent);
                }
            }
            // add all the target section's dependencies at once
            target_sec.lock().sections_i_depend_on.append(&mut target_sec_dependencies);
        }
        // here, we're done with handling all the relocations in this entire crate

        // Two final remaining tasks before the new crate is ready to go:
        // 1) remapping each section's mapped pages to the proper permission bits, since we initially mapped them all as writable
        // 2) give the new crate ownership of the loaded sections
        if let PageTable::Active(ref mut active_table) = kernel_mmi.page_table {
            let mut new_crate_locked = new_crate.write();
            // since we initially mapped the pages as writable, we need to remap them properly according to each section
            if let Some(ref mut tp) = new_crate_locked.text_pages { 
                try!(tp.remap(active_table, EntryFlags::PRESENT)); // present and executable (not no_execute)
            }
            if let Some(ref mut rp) = new_crate_locked.rodata_pages { 
                try!(rp.remap(active_table, EntryFlags::PRESENT | EntryFlags::NO_EXECUTE)); // present (just readable)
            }
            if let Some(ref mut dp) = new_crate_locked.data_pages { 
                try!(dp.remap(active_table, EntryFlags::PRESENT | EntryFlags::WRITABLE | EntryFlags::NO_EXECUTE)); // read/write
            }

            let (_keys, values): (Vec<usize>, Vec<StrongSectionRef>) = loaded_sections.into_iter().unzip();
            new_crate_locked.sections = values;
        }
        else {
            return Err("couldn't get kernel's active page table");
        }

        Ok(new_crate)
    }


    /// Adds any global symbols in the given list of `sections` to this namespace's symbol map.
    /// Returns the number of *new* unique global symbols added.
    /// 
    /// # Note
    /// If a symbol already exists in the symbol map, this leaves it intact and *does not* replace it.
    fn add_symbols<'a, I>(
        &self, 
        sections: I,
        crate_name: &str,
        log_replacements: bool
    ) -> usize 
        where I: IntoIterator<Item = &'a StrongSectionRef> 
    {
        let mut existing_map = self.symbol_map.lock();
        let new_map = metadata::global_symbol_map(sections, crate_name);

        // We *could* just use `append()` here, but that wouldn't let us know which entries
        // in the symbol map were being overwritten, which is currently a valuable bit of debugging info that we need.
        // Proper way for the future:  existing_map.append(&mut new_map);

        // add all the global symbols to the symbol map, in a way that lets us inspect/log each one
        let mut count = 0;
        for (key, new_sec) in new_map {
            // instead of blindly replacing old symbols with their new version, we leave all old versions intact 
            // TODO NOT SURE IF THIS IS THE CORRECT WAY, but blindly replacing them all is definitely wrong
            // The correct way is probably to use the hash values to disambiguate, but then we have to ensure deterministic/persistent hashes across different compilations
            let entry = existing_map.entry(key.clone());
            match entry {
                Entry::Occupied(old_val) => {
                    if let (Some(new_sec), Some(old_sec)) = (new_sec.upgrade(), old_val.get().upgrade()) {
                        let new_sec_size = new_sec.lock().size;
                        let old_sec_size = old_sec.lock().size;
                        if old_sec_size == new_sec_size {
                            if log_replacements { info!("       add_symbols \"{}\": Ignoring new symbol already present: {}", crate_name, key); }
                        }
                        else {
                            if log_replacements { 
                                warn!("       add_symbols \"{}\": unexpected: different section sizes (old={}, new={}), ignoring new symbol: {}", 
                                    crate_name, old_sec_size, new_sec_size, key);
                            }
                        }
                    }
                }
                Entry::Vacant(new) => {
                    new.insert(new_sec);
                    count += 1;
                }
            }
        }
        count
    }


    /// Removes any symbols that match the given `sections`' names from this namespace's symbol map.
    /// 
    /// Returns the number of symbols removed.
    fn remove_symbols<'a, I>(
        &self, 
        sections: I,
        log_removals: bool
    ) -> usize 
        where I: IntoIterator<Item = &'a StrongSectionRef> 
    {
        let mut count = 0;
        let mut existing_map = self.symbol_map.lock();
        for sec_ref in sections {
            let ref sec_name = sec_ref.lock().name;
            if existing_map.remove(sec_name).is_some() {
                count += 1;
                if log_removals {
                    debug!("  removed symbol: {}", sec_name);
                }
            }
        }
        count
    }



    fn replace_symbol_key(&self, old_symbol_key: &str, new_symbol_key: &str) -> Result<(), &'static str> {
        let mut existing_map = self.symbol_map.lock();
        if let Some(symbol_value) = existing_map.remove(old_symbol_key) {
            if existing_map.insert(new_symbol_key.to_string(), symbol_value).is_some() {
                warn!("replace_symbol_key(): overwrote existing symbol with new key \"{}\"", new_symbol_key);
            }
            Ok(())
        }
        else {
            error!("replace_symbol_key(): couldn't find old_symbol_key {:?} in symbol map (new: {:?})",
                old_symbol_key, new_symbol_key);
            Err("replace_symbol_key(): couldn't find old_symbol_key in symbol map")
        }
    }


    /// A convenience function that returns a weak reference to the `LoadedSection`
    /// that matches the given name (`demangled_full_symbol`), if it exists in the symbol map.
    /// Otherwise, it returns None if the symbol does not exist.
    fn get_symbol_internal(&self, demangled_full_symbol: &str) -> Option<WeakSectionRef> {
        self.symbol_map.lock().get(demangled_full_symbol).cloned()
    }


    /// Finds the corresponding `LoadedSection` reference for the given fully-qualified symbol string.
    pub fn get_symbol(&self, demangled_full_symbol: &str) -> WeakSectionRef {
        self.get_symbol_internal(demangled_full_symbol)
            .unwrap_or_else(|| Weak::default())
    }


    /// Finds the corresponding `LoadedSection` reference for the given fully-qualified symbol string,
    /// similar to the simpler function `get_symbol()`.
    /// 
    /// If the symbol cannot be found, it tries to load the kernel crate containing that symbol. 
    /// This can only be done for symbols that have a leading crate name, such as "my_crate::foo";
    /// if a symbol was given the `no_mangle` attribute, then we will not be able to find it
    /// and that symbol's containing crate should be manually loaded before invoking this. 
    /// 
    /// # Arguments
    /// * `demangled_full_symbol`: a fully-qualified symbol string, e.g., "my_crate::MyStruct::do_foo".
    /// * `backup_namespace`: the `CrateNamespace` that should be searched for missing symbols 
    ///   (for relocations) if a symbol cannot be found in this `CrateNamespace`. 
    ///   For example, the default namespace could be used by passing in `Some(get_default_namespace())`.
    ///   If `backup_namespace` is `None`, then no other namespace will be searched, 
    ///   and any missing symbols will return an `Err`. 
    /// * `kernel_mmi`: a mutable reference to the kernel's `MemoryManagementInfo`.
    pub fn get_symbol_or_load(
        &self, 
        demangled_full_symbol: &str, 
        backup_namespace: Option<&CrateNamespace>, 
        kernel_mmi: &mut MemoryManagementInfo,
        verbose_log: bool
    ) -> WeakSectionRef {
        // first, see if the section for the given symbol is already available and loaded
        if let Some(sec) = self.get_symbol_internal(demangled_full_symbol) {
            return sec;
        }
        // If not, our second try is to check the backup_namespace
        // to see if that namespace already has the section we want
        if let Some(weak_sec) = backup_namespace.and_then(|backup| backup.get_symbol_internal(demangled_full_symbol)) {
            // If we found it in the backup_namespace, then that saves us the effort of having to load the crate again.
            // We need to add a strong reference to that section's parent crate to this namespace as well, 
            // so it can't be dropped while this namespace is still relying on it.  
            if let Some(parent_crate) = weak_sec.upgrade().and_then(|sec| sec.lock().parent_crate.upgrade()) {
                let crate_name = parent_crate.read().crate_name.clone();
                self.add_symbols(&parent_crate.read().sections, &crate_name, verbose_log);
                self.crate_tree.lock().insert(crate_name, parent_crate);
                return weak_sec;
            }
            else {
                error!("get_symbol_or_load(): found symbol \"{}\" in backup namespace, but unexpectedly couldn't get its section's parent crate!",
                    demangled_full_symbol);
                return Weak::default();
            }
        }

        // If we couldn't get the symbol, then we attempt to load the kernel crate containing that symbol.
        // We are only able to do this for mangled symbols, those that have a leading crate name,
        // such as "my_crate::foo". 
        // If "foo()" was marked no_mangle, then we don't know which crate to load. 
        if let Some(crate_dependency_name) = get_parent_crate_name(demangled_full_symbol) {
            info!("Symbol \"{}\" not initially found, attemping to load its containing crate {:?}", 
                demangled_full_symbol, crate_dependency_name);
            
            // module names have a prefix like "__k_", so we need to prepend that to the crate name
            let crate_dependency_name = format!("{}{}", CrateType::Kernel.prefix(), crate_dependency_name);

            if let Some(dependency_module) = get_module(&crate_dependency_name) {
                // try to load the missing symbol's containing crate
                if let Ok(_num_new_syms) = self.load_kernel_crate(dependency_module, backup_namespace, kernel_mmi, verbose_log) {
                    // try again to find the missing symbol, now that we've loaded the missing crate
                    if let Some(sec) = self.get_symbol_internal(demangled_full_symbol) {
                        return sec;
                    }
                    else {
                        error!("Symbol \"{}\" not found, even after loading its containing crate \"{}\". Is that symbol actually in the crate?", 
                            demangled_full_symbol, crate_dependency_name);                                                        
                    }
                }
            }
            else {
                error!("Symbol \"{}\" not found, and couldn't find its containing crate's module \"{}\".", 
                    demangled_full_symbol, crate_dependency_name);
            }
        }
        else {
            error!("Symbol \"{}\" not found, cannot determine its containing crate (no leading crate name). Try loading the crate manually first.", 
                demangled_full_symbol);
        }

        // effectively the same as returning None, since it must be upgraded to an Arc before being used
        Weak::default()
    }

    
    /// Simple debugging function that returns the entire symbol map as a String.
    pub fn dump_symbol_map(&self) -> String {
        use core::fmt::Write;
        let mut output: String = String::new();
        let sysmap = self.symbol_map.lock();
        match write!(&mut output, "{:?}", sysmap.keys().collect::<Vec<&String>>()) {
            Ok(_) => output,
            _ => String::from("error"),
        }
    }

}


/// Crate names must be only alphanumeric characters, an underscore, or a dash. 
/// See: <https://www.reddit.com/r/rust/comments/4rlom7/what_characters_are_allowed_in_a_crate_name/>
fn is_valid_crate_name_char(c: char) -> bool {
    char::is_alphanumeric(c) || 
    c == '_' || 
    c == '-'
}

/// Parses the given symbol string to try to find the contained parent crate
/// that contains the symbol. 
/// If the parent crate cannot be determined (e.g., a `no_mangle` symbol),
/// then the input string is returned unchanged.
/// # Examples
/// ```
/// <*const T as core::fmt::Debug>::fmt   -->  core 
/// <alloc::boxed::Box<T>>::into_unique   -->  alloc
/// keyboard::init                        -->  keyboard
/// ```
fn get_parent_crate_name<'a>(demangled_full_symbol: &'a str) -> Option<&'a str> {
    demangled_full_symbol.split("::").next().and_then(|s| {
        // Get the last word right before the first "::"
        s.rsplit(|c| !is_valid_crate_name_char(c))
            .next() // the first element of the iterator (last element before the "::")
    })
}


/// Same as [`get_parent_crate_name()`](#method.get_parent_crate_name),
/// but replaces the parent crate name with the given `new_crate_name`, 
/// if it can be found, and if the parent crate name matches the `old_crate_name`. 
/// If the parent crate name can be found but does not match the expected `old_crate_name`,
/// then None is returned.
/// 
/// This creates an entirely new String rather than performing an in-place replacement, 
/// because the `new_crate_name` might be a different length than the original crate name.
/// # Examples
/// `keyboard::init  -->  keyboard_new::init`
fn replace_parent_crate_name<'a>(demangled_full_symbol: &'a str, old_crate_name: &str, new_crate_name: &str) -> Option<String> {
    // debug!("replace_parent_crate_name(dfs: {:?}, old: {:?}, new: {:?})", demangled_full_symbol, old_crate_name, new_crate_name);
    demangled_full_symbol.match_indices("::").next().and_then(|(index_of_first_double_colon, _substr)| {
        // Get the last word right before the first "::"
        demangled_full_symbol.get(.. index_of_first_double_colon).and_then(|substr| {
            // debug!("  replace_parent_crate_name(\"{}\"): substr: \"{}\" at index {}", demangled_full_symbol, substr, index_of_first_double_colon);
            let start_idx = substr.rmatch_indices(|c| !is_valid_crate_name_char(c))
                .next() // the first element right before the crate name starts
                .map(|(index_right_before_parent_crate_name, _substr)| { index_right_before_parent_crate_name + 1 }) // advance to the actual start 
                .unwrap_or(0); // if we didn't find any match, it means that everything since the beginning of the string was a valid char part of the parent_crate_name 

            demangled_full_symbol.get(start_idx .. index_of_first_double_colon)
                .filter(|&parent_crate_name| { parent_crate_name == old_crate_name })
                .and_then(|_parent_crate_name| {
                    // debug!("    replace_parent_crate_name(\"{}\"): parent_crate_name: \"{}\" at index [{}-{})", demangled_full_symbol, _parent_crate_name, start_idx, index_of_first_double_colon);
                    demangled_full_symbol.get(.. start_idx).and_then(|before_crate_name| {
                        demangled_full_symbol.get(index_of_first_double_colon ..).map(|after_crate_name| {
                            format!("{}{}{}", before_crate_name, new_crate_name, after_crate_name)
                        })
                    })
            })
        })
    })
}


struct PartiallyLoadedCrate<'e> {
    loaded_sections: BTreeMap<usize, StrongSectionRef>,
    elf_file: ElfFile<'e>,
    new_crate: StrongCrateRef,
}




/// A convenience wrapper for a set of the three possible types of `MappedPages`
/// that can be allocated and mapped for a single `LoadedCrate`. 
struct SectionPages {
    /// MappedPages that cover all .text sections, if any exist.
    text_pages:   Option<MappedPages>, //Option<Arc<Mutex<MappedPages>>>,
    /// MappedPages that cover all .rodata sections, if any exist.
    rodata_pages: Option<MappedPages>, //Option<Arc<Mutex<MappedPages>>>,
    /// MappedPages that cover all .data and .bss sections, if any exist.
    data_pages:   Option<MappedPages>, //Option<Arc<Mutex<MappedPages>>>,
}


/// Allocates enough space for the sections that are found in the given `ElfFile`.
/// Returns a tuple of `MappedPages` for the .text, .rodata, and .data/.bss sections, in that order.
fn allocate_section_pages(elf_file: &ElfFile, kernel_mmi: &mut MemoryManagementInfo) 
    -> Result<SectionPages, &'static str> 
{
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

    // create a closure here to allocate N contiguous virtual memory pages
    // and map them to random frames as writable, returns Result<MappedPages, &'static str>
    let (text_pages, rodata_pages, data_pages): (Option<MappedPages>, Option<MappedPages>, Option<MappedPages>) = {
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
            if text_bytecount   > 0 { allocate_pages_closure(text_bytecount).ok()   } else { None }, 
            if rodata_bytecount > 0 { allocate_pages_closure(rodata_bytecount).ok() } else { None }, 
            if data_bytecount   > 0 { allocate_pages_closure(data_bytecount).ok()   } else { None }
        )
    };

    Ok(
        SectionPages {
            text_pages,
            rodata_pages,
            data_pages,
            // text_pages:   text_pages  .map(|tp| Arc::new(Mutex::new(tp))), 
            // rodata_pages: rodata_pages.map(|rp| Arc::new(Mutex::new(rp))),
            // data_pages:   data_pages  .map(|dp| Arc::new(Mutex::new(dp))),
        }
    )
}


/// Maps the given `ModuleArea` for a crate and returns the `MappedPages` that contain it. 
fn map_crate_module(crate_module: &ModuleArea, mmi: &mut MemoryManagementInfo) -> Result<MappedPages, &'static str> {
    use kernel_config::memory::address_is_page_aligned;
    if !address_is_page_aligned(crate_module.start_address()) {
        error!("map_crate_module(): crate_module {} is not page aligned!", crate_module.name());
        return Err("map_crate_module(): crate_module is not page aligned");
    } 

    // first we need to map the module memory region into our address space, 
    // so we can then parse the module as an ELF file in the kernel.
    if let PageTable::Active(ref mut active_table) = mmi.page_table {
        let new_pages = allocate_pages_by_bytes(crate_module.size()).ok_or("couldn't allocate pages for crate module")?;
        let mut frame_allocator = FRAME_ALLOCATOR.try().ok_or("couldn't get FRAME_ALLOCATOR")?.lock();
        active_table.map_allocated_pages_to(
            new_pages, 
            Frame::range_inclusive_addr(crate_module.start_address(), 
            crate_module.size()), 
            EntryFlags::PRESENT, 
            frame_allocator.deref_mut()
        )
    }
    else {
        error!("map_crate_module(): error getting kernel's active page table to temporarily map crate_module {}.", crate_module.name());
        Err("map_crate_module(): couldn't get kernel's active page table")
    }
}


/// Write the actual relocation entry.
/// # Arguments
/// * `rela_entry`: the relocation entry from the ELF file that specifies which relocation action to perform.
/// * `target_sec`: the target section of the relocation, i.e., the section where the relocation data will be written to.
/// * `target_sec_parent_crate`: the parent crate of the `target_sec`, which is an optional performance enhancement 
///    that prevents this function from having to find the `target_sec`'s parent crate every time.
/// * `source_sec`: the source section of the relocation, i.e., the section that the `target_sec` depends on and "points" to.
/// * `kernel_mmi`: the `MemoryManagementInfo`  
/// * `verbose_log`: whether to output verbose logging information about this relocation action.
/// 
/// # Returns
/// If the `target_sec` and `source_sec` are from the same parent `LoadedCrate`, then `None` is returned.
/// 
/// If they are from different parent crates, then a tuple of a `StrongDependency` and a `WeakDependent` is returned.
/// The `StrongDependency` should be added to the `target_sec.sections_i_depend_on`, 
/// whereas the `WeakDependent` should be added to the `source_sec.sections_dependent_on_me`.
fn write_relocation(
    relocation_entry: RelocationEntry, 
    target_sec: &StrongSectionRef,
    target_sec_parent_crate: Option<StrongCrateRef>,
    source_sec: &StrongSectionRef,
    kernel_mmi: &mut MemoryManagementInfo,
    verbose_log: bool
) -> Result<Option<(StrongDependency, WeakDependent)>, &'static str>
{
    let target_sec_parent_crate = match target_sec_parent_crate {
        Some(pc) => pc,
        _ => target_sec.lock().parent_crate.upgrade().ok_or("Couldn't get target_sec's parent_crate")?,
    };

    let source_sec_parent_crate = source_sec.lock().parent_crate.upgrade().ok_or("Couldn't get source_sec's parent_crate")?;
    let source_and_target_have_same_parent_crate = Arc::ptr_eq(&target_sec_parent_crate, &source_sec_parent_crate);

    let source_sec_vaddr = {
        let parent_crate = source_sec_parent_crate.read();
        let source_sec_locked = source_sec.lock();
        let source_mapped_pages = source_sec_locked.mapped_pages(&parent_crate)
            .ok_or("couldn't get source_sec's MappedPages (in its parent_crate) for relocation")?;
        source_mapped_pages.address_at_offset(source_sec_locked.mapped_pages_offset)
            .ok_or("couldn't get source_sec's VirtualAddress for relocation")?
    };

    let mut target_sec_parent_crate_locked = target_sec_parent_crate.write();
    let (target_sec_mapped_pages, target_sec_mapped_pages_offset) = {
        let mut locked_target = target_sec.lock();
        (
            locked_target.mapped_pages_mut(&mut target_sec_parent_crate_locked).ok_or("couldn't get target_sec's MappedPages for relocation")?,
            locked_target.mapped_pages_offset
        )
    };

    // If the target_sec's mapped pages aren't writable (which is common in the case of swapping),
    // then we need to temporarily remap them as writable here so we can fix up the target_sec's new relocation entry.
    let target_sec_initial_flags = target_sec_mapped_pages.flags();
    if !target_sec_initial_flags.is_writable() {
        if let PageTable::Active(ref mut active_table) = kernel_mmi.page_table {
            target_sec_mapped_pages.remap(active_table, target_sec_initial_flags | EntryFlags::WRITABLE)?;
        }
        else {
            return Err("couldn't get kernel's active page table");
        }
    }

    // Calculate exactly where we should write the relocation data to.
    let target_offset = target_sec_mapped_pages_offset + relocation_entry.offset;

    // Perform the actual relocation data writing here.
    // There is a great, succint table of relocation types here
    // https://docs.rs/goblin/0.0.13/goblin/elf/reloc/index.html
    match relocation_entry.typ {
        R_X86_64_32 => {
            let target_ref: &mut u32 = try!(target_sec_mapped_pages.as_type_mut(target_offset));
            let source_val = source_sec_vaddr.wrapping_add(relocation_entry.addend);
            if verbose_log { trace!("                    target_ptr: {:#X}, source_val: {:#X} ({:?})", target_ref as *mut _ as usize, source_val, source_sec.lock().name); }
            *target_ref = source_val as u32;
        }
        R_X86_64_64 => {
            let target_ref: &mut u64 = try!(target_sec_mapped_pages.as_type_mut(target_offset));
            let source_val = source_sec_vaddr.wrapping_add(relocation_entry.addend);
            if verbose_log { trace!("                    target_ptr: {:#X}, source_val: {:#X} ({:?})", target_ref as *mut _ as usize, source_val, source_sec.lock().name); }
            *target_ref = source_val as u64;
        }
        R_X86_64_PC32 => {
            let target_ref: &mut u32 = try!(target_sec_mapped_pages.as_type_mut(target_offset));
            let source_val = source_sec_vaddr.wrapping_add(relocation_entry.addend).wrapping_sub(target_ref as *mut _ as usize);
            if verbose_log { trace!("                    target_ptr: {:#X}, source_val: {:#X} ({:?})", target_ref as *mut _ as usize, source_val, source_sec.lock().name); }
            *target_ref = source_val as u32;
        }
        R_X86_64_PC64 => {
            let target_ref: &mut u64 = try!(target_sec_mapped_pages.as_type_mut(target_offset));
            let source_val = source_sec_vaddr.wrapping_add(relocation_entry.addend).wrapping_sub(target_ref as *mut _ as usize);
            if verbose_log { trace!("                    target_ptr: {:#X}, source_val: {:#X} ({:?})", target_ref as *mut _ as usize, source_val, source_sec.lock().name); }
            *target_ref = source_val as u64;
        }
        // R_X86_64_GOTPCREL => { 
        //     unimplemented!(); // if we stop using the large code model, we need to create a Global Offset Table
        // }
        _ => {
            error!("found unsupported relocation type {}\n  --> Are you compiling crates with 'code-model=large'?", relocation_entry.typ);
            return Err("found unsupported relocation type. Are you compiling crates with 'code-model=large'?");
        }
    }

    // If we temporarily remapped the target_sec's mapped pages as writable, undo that here
    if !target_sec_initial_flags.is_writable() {
        if let PageTable::Active(ref mut active_table) = kernel_mmi.page_table {
            target_sec_mapped_pages.remap(active_table, target_sec_initial_flags)?;
        }
        else {
            return Err("couldn't get kernel's active page table");
        }
    }

    // We only care about recording dependency information if the source and target sections are in different crates
    if source_and_target_have_same_parent_crate {
        Ok(None)
    }
    else {
        // tell the source_sec that the target_sec is dependent upon it
        let weak_dependent = WeakDependent {
            section: Arc::downgrade(target_sec),
            relocation: relocation_entry,
        };
        
        // tell the target_sec that it has a strong dependency on the source_sec
        let strong_dependency = StrongDependency {
            section: source_sec.clone(),
            relocation: relocation_entry,
        };          

        Ok(Some((strong_dependency, weak_dependent)))
    }
}





fn dump_dependent_crates(krate: &LoadedCrate, prefix: String) {
	for weak_crate_ref in krate.crates_dependent_on_me() {
		let strong_crate_ref = weak_crate_ref.upgrade().unwrap();
		let locked_crate = strong_crate_ref.read();
		debug!("{}{}", prefix, locked_crate.crate_name);
		dump_dependent_crates(&*locked_crate, format!("{}  ", prefix));
	}
}


fn dump_weak_dependents(sec: &LoadedSection, prefix: String) {
	if !sec.sections_dependent_on_me.is_empty() {
		debug!("{}Section \"{}\": sections dependent on me (weak dependents):", prefix, sec.name);
		for weak_dep in &sec.sections_dependent_on_me {
			if let Some(wds) = weak_dep.section.upgrade() {
				let prefix = format!("{}  ", prefix); // add two spaces of indentation to the prefix
				dump_weak_dependents(&*wds.lock(), prefix);
			}
			else {
				debug!("{}ERROR: weak dependent failed to upgrade()", prefix);
			}
		}
	}
	else {
		debug!("{}Section \"{}\"  (no weak dependents)", prefix, sec.name);
	}
}



 

/// This function first loads all of the given crates into a new, separate namespace in isolation,
/// and only after *all* crates are loaded does it move on to linking/relocation calculations. 
/// This allows them to be linked against each other first, rather than to always fall back to
/// linking against existing symbols in this namespace, so this namespace serves as the `backup_namespace`. 
/// It is this isolated preloading of crate sections that allows us to create a package of crates
/// that are all new and can be swapped as a single unit. 
fn load_crates_in_new_namespace<'m, I>(
	new_modules: I,
	backup_namespace: &CrateNamespace,
	kernel_mmi: &mut MemoryManagementInfo,
	verbose_log: bool,
) -> Result<CrateNamespace, &'static str> 
	where I: Iterator<Item = &'m ModuleArea> + Clone 
{
	// first we map all of the crates' ModuleAreas
	let mappings = {
		let mut mappings: Vec<MappedPages> = Vec::new(); //Vec::with_capacity(len);
		for crate_module in new_modules.clone() {
			mappings.push(map_crate_module(crate_module, kernel_mmi)?);
		}
		mappings
	};

	// create a new empty namespace so we can add symbols to it before performing the relocations
	let new_namespace = CrateNamespace::new();
	let mut partially_loaded_crates: Vec<PartiallyLoadedCrate> = Vec::with_capacity(mappings.len()); 

	// first we do all of the section parsing and loading
	for (i, crate_module) in new_modules.clone().enumerate() {
		let temp_module_mapping = mappings.get(i).ok_or("Fatal logic error: mapped crate module successfully but couldn't retrieve mapping (WTF?)")?;
		let plc = new_namespace.load_crate_sections(temp_module_mapping, crate_module.size(), crate_module.name(), kernel_mmi, verbose_log)?;
		let _new_syms = new_namespace.add_symbols(plc.loaded_sections.values(), &plc.new_crate.read().crate_name.clone(), verbose_log);
		partially_loaded_crates.push(plc);
	}
	
	// then we do all of the relocations 
	for plc in partially_loaded_crates {
		let new_crate = new_namespace.perform_relocations(&plc.elf_file, plc.new_crate, plc.loaded_sections, Some(backup_namespace), kernel_mmi, verbose_log)?;
		let name = new_crate.read().crate_name.clone();
		new_namespace.crate_tree.lock().insert(name, new_crate);
	}

	Ok(new_namespace)
}



/// Returns a reference to the symbol table in the given `ElfFile`.
fn find_symbol_table<'e>(elf_file: &'e ElfFile) 
    -> Result<&'e [xmas_elf::symbol_table::Entry64], &'static str>
    {
    use xmas_elf::sections::SectionData::SymbolTable64;
    let symtab_data = elf_file.section_iter()
        .filter(|sec| sec.get_type() == Ok(ShType::SymTab))
        .next()
        .ok_or("no symtab section")
        .and_then(|s| s.get_data(&elf_file));

    match symtab_data {
        Ok(SymbolTable64(symtab)) => Ok(symtab),
        _ => {
            Err("no symbol table found. Was file stripped?")
        }
    }
}
