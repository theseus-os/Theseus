#![no_std]
#![feature(alloc)]
#![feature(rustc_private)]
#![feature(transpose_result)]

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
extern crate cow_arc;


use core::ops::DerefMut;
use alloc::{Vec, BTreeMap, BTreeSet, String};
use alloc::string::ToString;
use alloc::btree_map::Entry;
use alloc::arc::{Arc, Weak};
use spin::Mutex;

use xmas_elf::ElfFile;
use xmas_elf::sections::{SectionData, ShType};
use xmas_elf::sections::{SHF_WRITE, SHF_ALLOC, SHF_EXECINSTR};
use goblin::elf::reloc::*;

use util::round_up_power_of_two;
use memory::{FRAME_ALLOCATOR, get_module, MemoryManagementInfo, ModuleArea, Frame, PageTable, VirtualAddress, MappedPages, EntryFlags, allocate_pages_by_bytes};

use metadata::{StrongCrateRef, WeakSectionRef};
use cow_arc::CowArc;


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
    static ref DEFAULT_CRATE_NAMESPACE: CrateNamespace = CrateNamespace::with_name("default");
}

pub fn get_default_namespace() -> &'static CrateNamespace {
    &DEFAULT_CRATE_NAMESPACE
}

/// This should be a const, but Rust doesn't like OR-ing bitflags as a const expression.
#[allow(non_snake_case)]
pub fn TEXT_SECTION_FLAGS() -> EntryFlags {
    EntryFlags::PRESENT
}
/// This should be a const, but Rust doesn't like OR-ing bitflags as a const expression.
#[allow(non_snake_case)]
pub fn RODATA_SECTION_FLAGS() -> EntryFlags {
    EntryFlags::PRESENT | EntryFlags::NO_EXECUTE
}
/// This should be a const, but Rust doesn't like OR-ing bitflags as a const expression.
#[allow(non_snake_case)]
pub fn DATA_BSS_SECTION_FLAGS() -> EntryFlags {
    EntryFlags::PRESENT | EntryFlags::NO_EXECUTE | EntryFlags::WRITABLE
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
    /// An identifier for this namespace, just for convenience.
    name: String,

    /// The list of all the crates in this namespace,
    /// stored as a map in which the crate's String name
    /// is the key that maps to the value, a strong reference to a crate.
    /// It is a strong reference because a crate must not be removed
    /// as long as it is part of any namespace,
    /// and a single crate can be part of multiple namespaces at once.
    /// For example, the "core" (Rust core library) crate is essentially
    /// part of every single namespace, simply because most other crates rely upon it. 
    pub crate_tree: Mutex<BTreeMap<String, StrongCrateRef>>,

    /// The "system map" of all global (publicly-visible) symbols
    /// that are present in all of the crates in this `CrateNamespace`.
    /// Maps a fully-qualified symbol name string to a corresponding `LoadedSection`,
    /// which is guaranteed to be part of one of the crates in this `CrateNamespace`.  
    /// Symbols declared as "no_mangle" will appear in the map with no crate prefix, as expected.
    symbol_map: Mutex<SymbolMap>,
}

impl CrateNamespace {
    /// Creates a new `CrateNamespace` that is completely empty. 
    pub fn new() -> CrateNamespace {
        CrateNamespace {
            name: String::from(""),
            crate_tree: Mutex::new(BTreeMap::new()),
            symbol_map: Mutex::new(SymbolMap::new()),
        }
    } 
    

    /// Creates a new `CrateNamespace` that is completely empty, and is given the specified `name`.
    pub fn with_name(name: &str) -> CrateNamespace {
        CrateNamespace {
            name: String::from(name),
            crate_tree: Mutex::new(BTreeMap::new()),
            symbol_map: Mutex::new(SymbolMap::new()),
        }
    } 


    /// Returns a list of all of the crate names currently loaded into this `CrateNamespace`.
    pub fn crate_names(&self) -> Vec<String> {
        self.crate_tree.lock().keys().cloned().collect()
    }


    /// Acquires the lock on this `CrateNamespace`'s crate list and looks for the crate 
    /// that matches the given `crate_name`, if it exists in this namespace.
    /// 
    /// Returns a `StrongCrateReference` that **has not** been marked as a shared crate reference,
    /// so if the caller wants to keep the returned `StrongCrateRef` as a shared crate 
    /// that jointly exists in another namespace, they should invoke the 
    /// [`CowArc::share()`](cow_arc/CowArc.share.html) function on the returned value.
    pub fn get_crate(&self, crate_name: &str) -> Option<StrongCrateRef> {
        self.crate_tree.lock().get(crate_name).map(|r| CowArc::clone_shallow(r))
    }


    /// Loads the specified application crate into memory, allowing it to be invoked.
    /// 
    /// The argument `load_symbols_as_singleton` determines what is done with the newly-loaded crate:      
    /// * If true, this application is loaded as a system-wide singleton crate and added to this namespace, 
    ///   and its public symbols are added to this namespace's symbol map, allowing other applications to depend upon it.
    /// * If false, this application is loaded normally and not added to this namespace in any way,
    ///   allowing it to be loaded again in the future as a totally separate instance.
    /// 
    /// Returns a Result containing the new crate itself.
    pub fn load_application_crate(
        &self, 
        crate_module: &ModuleArea, 
        kernel_mmi: &mut MemoryManagementInfo, 
        load_symbols_as_singleton: bool,
        verbose_log: bool
    ) -> Result<StrongCrateRef, &'static str> {
        let (crate_type, crate_name) = CrateType::from_module_name(crate_module.name())?;
        if crate_type != CrateType::Application {
            error!("load_application_crate() cannot be used for crate \"{}\", only for application crate modules starting with \"{}\"",
                crate_module.name(), CrateType::Application.prefix());
            return Err("load_application_crate() can only be used for application crate modules");
        }
        
        debug!("load_application_crate: trying to load \"{}\" application module", crate_module.name());
        let temp_module_mapping = map_crate_module(crate_module, kernel_mmi)?;
        let (new_crate_ref, elf_file) = self.load_crate_sections(&temp_module_mapping, crate_module.size(), crate_name, kernel_mmi, verbose_log)?;
        
        // no backup namespace when loading applications, they must be able to find all symbols in only this namespace (&self)
        self.perform_relocations(&elf_file, &new_crate_ref, None, kernel_mmi, verbose_log)?;

        if load_symbols_as_singleton {
            // if this is a singleton application, we add its public symbols (except "main")
            let new_crate = new_crate_ref.lock_as_ref();
            self.add_symbols_filtered(new_crate.sections.values(), 
                |sec| sec.name != "main", 
                verbose_log
            );
            self.crate_tree.lock().insert(new_crate.crate_name.clone(), CowArc::clone_shallow(&new_crate_ref));
        } else {
            let new_crate = new_crate_ref.lock_as_ref();
            info!("loaded new application crate module: {}, num sections: {}", new_crate.crate_name, new_crate.sections.len());
        }
        Ok(new_crate_ref)

        // temp_module_mapping is automatically unmapped when it falls out of scope here (frame allocator must not be locked)
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

        let (crate_type, crate_name) = CrateType::from_module_name(crate_module.name())?;
        if crate_type != CrateType::Kernel {
            error!("load_kernel_crate() cannot be used for crate \"{}\", only for kernel crate modules starting with \"{}\"",
                crate_module.name(), CrateType::Kernel.prefix());
            return Err("load_kernel_crate() can only be used for kernel crate modules");
        }

        debug!("load_kernel_crate: trying to load \"{}\" kernel crate", crate_name);
        let temp_module_mapping = map_crate_module(crate_module, kernel_mmi)?;
        let (new_crate_ref, elf_file) = self.load_crate_sections(&temp_module_mapping, crate_module.size(), crate_name, kernel_mmi, verbose_log)?;
        self.perform_relocations(&elf_file, &new_crate_ref, backup_namespace, kernel_mmi, verbose_log)?;
        let (new_crate_name, new_syms) = {
            let new_crate = new_crate_ref.lock_as_ref();
            let new_syms = self.add_symbols(new_crate.sections.values(), verbose_log);
            (new_crate.crate_name.clone(), new_syms)
        };
            
        info!("loaded module {:?} as new crate {:?}, {} new symbols.", crate_module.name(), new_crate_name, new_syms);
        self.crate_tree.lock().insert(new_crate_name, new_crate_ref);
        Ok(new_syms)
        
        // plc.temp_module_mapping is automatically unmapped when it falls out of scope here (frame allocator must not be locked)
    }

    
    /// This function first loads all of the given crates' sections and adds them to the symbol map,
    /// and only after *all* crates are loaded does it move on to linking/relocation calculations. 
    /// 
    /// This allows multiple object files with circular dependencies on one another
    /// to be loaded "atomically", i.e., as a single unit. 
    /// 
    /// # Example
    /// If crate `A` depends on crate `B`, and crate `B` depends on crate `A`,
    /// this function will load both crate `A` and `B` before trying to resolve their dependencies individually. 
    pub fn load_kernel_crates<'m, I>(
        &self,
        new_modules: I,
        backup_namespace: Option<&CrateNamespace>,
        kernel_mmi: &mut MemoryManagementInfo,
        verbose_log: bool,
    ) -> Result<(), &'static str> 
        where I: Iterator<Item = &'m ModuleArea> + Clone
    {
        // first we map all of the crates' ModuleAreas
        let mappings = {
            let mut mappings: Vec<MappedPages> = Vec::new(); //Vec::with_capacity(len);
            for crate_module in new_modules.clone() {
                debug!("mapping crate_module {:?}", crate_module);
                mappings.push(map_crate_module(crate_module, kernel_mmi)?);
            }
            mappings
        };

        let mut partially_loaded_crates: Vec<(StrongCrateRef, ElfFile)> = Vec::with_capacity(mappings.len()); 

        // then we do all of the section parsing and loading
        for (i, crate_module) in new_modules.enumerate() {
            let temp_module_mapping = mappings.get(i).ok_or("BUG: mapped crate module successfully but couldn't retrieve mapping (WTF?)")?;
            let (new_crate, elf_file) = self.load_crate_sections(
                temp_module_mapping, 
                crate_module.size(),
                CrateType::from_module_name(crate_module.name())?.1,
                kernel_mmi, 
                verbose_log
            )?;
            let _new_syms = self.add_symbols(new_crate.lock_as_ref().sections.values(), verbose_log);
            partially_loaded_crates.push((new_crate, elf_file));
        }
        
        // then we do all of the relocations 
        for (new_crate_ref, elf_file) in partially_loaded_crates {
            self.perform_relocations(&elf_file, &new_crate_ref, backup_namespace, kernel_mmi, verbose_log)?;
            let name = new_crate_ref.lock_as_ref().crate_name.clone();
            self.crate_tree.lock().insert(name, new_crate_ref);
        }

        Ok(())
    }




    /// Duplicates this `CrateNamespace` into a new `CrateNamespace`, 
    /// but uses a copy-on-write/clone-on-write semantic that creates 
    /// a special shared reference to each crate that indicates it is shared across multiple namespaces.
    /// 
    /// In other words, crates in the new namespace returned by this fucntions 
    /// are fully shared with crates in *this* namespace, 
    /// until either namespace attempts to modify a shared crate in the future.
    /// 
    /// When modifying crates in the new namespace, such as with the funtions 
    /// [`swap_crates`](#method.swap_crates), any crates in the new namespace
    /// that are still shared with the old namespace will be deeply copied into a new crate,
    /// and then that new crate will be modified in whichever way specified. 
    /// For example, if you swapped one crate `A` in the new namespace returned from this function
    /// and loaded a new crate `A2` in its place,
    /// and two other crates `B` and `C` depended on that newly swapped-out `A`,
    /// then `B` and `C` would be transparently deep copied before modifying them to depend on
    /// the new crate `A2`, and you would be left with `B2` and `C2` as deep copies of `B` and `C`,
    /// that now depend on `A2` instead of `A`. 
    /// The existing versions of `B` and `C` would still depend on `A`, 
    /// but they would no longer be part of the new namespace. 
    /// 
    pub fn clone_on_write(&self) -> CrateNamespace {
        let new_crate_tree = self.crate_tree.lock().clone();
        let new_symbol_map = self.symbol_map.lock().clone();

        CrateNamespace {
            name: self.name.clone(),
            crate_tree: Mutex::new(new_crate_tree),
            symbol_map: Mutex::new(new_symbol_map),
        }
    }


    /// Swaps in new modules to replace existing crates this in `CrateNamespace`.
    /// This function accepts several modules in order to allow swapping multiple crates all at once 
    /// in a single "atomic" unit, which prevents weird linking/relocation errors, 
    /// such as a new crate linking against an old crate that already exists in this namespace
    /// instead of linking against the new one that we want to replace that old crate with. 
    /// 
    /// In general, the strategy for replacing and old module `C` with a new module `C2` consists of three simple steps:
    /// 1) Load the new replacement module `C2`.
    /// 2) Set up new relocation entries that redirect all module's dependencies on the old module `C` to the new module `C2`.
    /// 3) Remove module `C` and clean it up, e.g., removing its entries from the symbol map.
    /// 
    /// This `CrateNamespace` (self) is used as the backup namespace for resolving unknown symbols.
    /// 
    /// Upon a successful return, this namespace (self) will have the new crates in place of the old ones,
    /// and the old crates will be completely removed from this namespace. 
    /// 
    /// # Note
    /// This function currently makes no attempt to guarantee correct operation after a crate is swapped. 
    /// For example, if the new crate changes a function or data structure, there is no guarantee that 
    /// other crates will be swapped alongside that one -- that responsibility is currently left to the caller.
    pub fn swap_crates(
        &self,
        swap_pairs: Vec<(StrongCrateRef, &ModuleArea, Option<String>)>,
        kernel_mmi: &mut MemoryManagementInfo,
        verbose_log: bool,
    ) -> Result<(), &'static str> {
        // create a new CrateNamespace and load all of the new crate modules into it
        let new_namespace = {
            let nn = CrateNamespace::with_name(&format!("temp_swap--{:?}", swap_pairs));
            let module_iter = swap_pairs.iter().map(|tup| tup.1);
            nn.load_kernel_crates(module_iter, Some(self), kernel_mmi, verbose_log)?;
            nn
        };

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
            let old_crate = old_crate_ref.lock_as_mut().ok_or_else(|| {
                error!("TODO FIXME: swap_crates(), old_crate: {:?}, doesn't yet support deep copying shared crates to get a new exlusive mutable instance", old_crate_ref);
                "TODO FIXME: swap_crates() doesn't yet support deep copying shared crates to get a new exlusive mutable instance"
            })?;
            let (_new_crate_type, new_crate_name) = CrateType::from_module_name(new_module.name())?;
            debug!("trying to get_crate({})", new_crate_name);
            debug!("new_namespace crates: {:?}", &*new_namespace.crate_tree.lock());
            let new_crate_ref = new_namespace.get_crate(new_crate_name)
                .ok_or_else(|| "BUG: Couldn't get new crate that should've just been loaded into a new temporary namespace")?;
            let mut new_crate = new_crate_ref.lock_as_mut()
                .ok_or_else(|| "BUG: swap_crates(): new_crate was unexpectedly shared in another namespace (couldn't get as exclusively mutable)...?")?;

            // if requested, override the new crate's name and section prefixes and symbol prefixes
            let new_crate_name = if let Some(override_name) = override_new_crate_name {
                debug!("Overriding new crate name \"{}\" with \"{}\"", new_crate_name, override_name);
                for new_sec_ref in new_crate.sections.values() {
                    let mut new_sec = new_sec_ref.lock();
                    // debug!("  Looking at {:?} section \"{}#{:?}\" (global: {})", new_sec.typ, new_sec.name, new_sec.hash, new_sec.global);
                    if let Some(new_name) = replace_containing_crate_name(&new_sec.name, &new_crate.crate_name, &override_name) {
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
                // if no override name was supplied, then the new crate's name is derived from its module name.
                new_crate_name.to_string()
            };

            debug!("====================== Replacing old_crate \"{}\"  ===========================", old_crate.crate_name);

            
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
            for old_sec_ref in old_crate.sections.values() {

                let mut old_sec = old_sec_ref.lock();
                for weak_dep in &old_sec.sections_dependent_on_me {
                    let target_sec_ref = weak_dep.section.upgrade().ok_or_else(|| "couldn't upgrade WeakDependent.section")?;
                    let relocation_entry = weak_dep.relocation;

                    // Use the new namespace to find the new source_sec that old target_sec should point to.
                    // The new source_sec must have the same name as the old one (old_sec here),
                    // otherwise it wouldn't be a valid swap -- the target_sec's parent crate should have also been swapped.
                    // The new namespace should already have that symbol available (i.e., we shouldn't have to load it on demand);
                    // if not, the swapping action was never going to work and we shouldn't go through with it
                    let source_sec_ref = new_namespace.get_symbol(&old_sec.name).upgrade().ok_or_else(|| 
                        "couldn't find replacement source section in new namespace that the target section needs to point to, perhaps the target section must also be swapped?"
                    )?;
                    let mut source_sec = source_sec_ref.lock();

                    // If the target_sec's mapped pages aren't writable (which is common in the case of swapping),
                    // then we need to temporarily remap them as writable here so we can fix up the target_sec's new relocation entry.
                    let mut target_sec = target_sec_ref.lock();
                    {
                        let mut target_sec_mapped_pages = target_sec.mapped_pages.lock();
                        let target_sec_initial_flags = target_sec_mapped_pages.flags();
                        if !target_sec_initial_flags.is_writable() {
                            if let PageTable::Active(ref mut active_table) = kernel_mmi.page_table {
                                target_sec_mapped_pages.remap(active_table, target_sec_initial_flags | EntryFlags::WRITABLE)?;
                            }
                            else {
                                return Err("couldn't get kernel's active page table");
                            }
                        }

                        write_relocation(
                            relocation_entry, 
                            &mut target_sec_mapped_pages, 
                            target_sec.mapped_pages_offset, 
                            source_sec.virt_addr(), 
                            true || verbose_log
                        )?;

                        // If we temporarily remapped the target_sec's mapped pages as writable, undo that here
                        if !target_sec_initial_flags.is_writable() {
                            if let PageTable::Active(ref mut active_table) = kernel_mmi.page_table {
                                target_sec_mapped_pages.remap(active_table, target_sec_initial_flags)?;
                            }
                            else {
                                return Err("couldn't get kernel's active page table");
                            }
                        };
                    }

                    // Tell the old source_sec that it no longer has the existing target_sec as a dependent
                        // this is performed all at once at the end of this loop: we clear the whole old_sec.sections_dependent_on_me
                    // Tell the new source_sec that the existing target_sec depends on it.
                    source_sec.sections_dependent_on_me.push(WeakDependent {
                        section: Arc::downgrade(&target_sec_ref),
                        relocation: relocation_entry,
                    });

                    // Tell the existing target_sec that it no longer depends on the old source section (old_sec_ref),
                    // and that it now depends on the new source_sec.
                    let index = target_sec.find_strong_dependency(&old_sec_ref)
                        .ok_or("Couldn't find/remove the target_sec's StrongDependency on the old crate section")?;
                    target_sec.sections_i_depend_on.remove(index);
                    target_sec.sections_i_depend_on.push(StrongDependency {
                        section: Arc::clone(&source_sec_ref),
                        relocation: relocation_entry,
                    });      
                }

                // As described in the above loop, we must tell the old section (the one we're replacing)
                // that no other sections rely on it. 
                old_sec.sections_dependent_on_me.clear();
            
                // Currently we are just copying over the old_sec into the new source_sec,
                // if they represent a static variable (state spill that would otherwise result in a loss of data).
                // Currently, static variables (states) are only .bss sections
                if old_sec.typ == SectionType::Bss {
                    let new_dest_sec = new_crate.find_section(|sec| sec.name == old_sec.name).ok_or_else(|| 
                        "couldn't find destination section in new crate for copying old_sec's data into (BSS state transfer)"
                    )?;
                    old_sec.copy_section_data_to(&mut new_dest_sec.lock())?;
                }
            }
            
            // remove the old crate from this namespace, and remove its sections' symbols too
            self.remove_symbols(old_crate.sections.values(), true);
            if self.crate_tree.lock().remove(&old_crate.crate_name).is_some() {
                info!("  Removed old crate {}", old_crate.crate_name);
            }

            // add the new crate and its sections' symbols to this namespace
            self.add_symbols(new_crate.sections.values(), verbose_log);
            self.crate_tree.lock().insert(new_crate_name, new_crate_ref.clone());
            // Although we used a shared CowArc above, it will go back to being exclusive after this function
            // because the `new_namespace` will be dropped.
        }

        Ok(())

        // here, "new_namespace is dropped, but its crates have already been added to the current namespace 
    }



    /// The primary internal routine for parsing and loading all of the sections.
    /// This does not perform any relocations or linking, so the crate **is not yet ready to use after this function**,
    /// since its sections are totally incomplete and non-executable.
    /// 
    /// However, it does add all of the newly-loaded crate sections to the symbol map (yes, even before relocation/linking),
    /// since we can use them to resolve missing symbols for relocations.
    /// 
    /// Parses each section in the given `ElfFile` and copies the object file contents to each section.
    /// Returns a tuple of the new `LoadedCrate`, the list of newly `LoadedSection`s, and the crate's ELF file.
    /// The list of sections is actually a map from its section index (shndx) to the `LoadedSection` itself,
    /// which is kept separate and has not yet been added to the new `LoadedCrate` beause it needs to be used for relocations.
    fn load_crate_sections<'e>(
        &self,
        mapped_pages: &'e MappedPages, 
        size_in_bytes: usize, 
        crate_name: &str, 
        kernel_mmi: &mut MemoryManagementInfo,
        _verbose_log: bool
    ) -> Result<(StrongCrateRef, ElfFile<'e>), &'static str> {
        
        // First, check to make sure this crate hasn't already been loaded. 
        // Regular, non-singleton application crates aren't added to the CrateNamespace, so they can be multiply loaded.
        if self.crate_tree.lock().contains_key(crate_name) {
            return Err("the crate has already been loaded, so it doesn't make sense to load it again into the same namespace");
        }

        // Parse the given `mapped_pages` as an ELF file
        let byte_slice: &[u8] = mapped_pages.as_slice(0, size_in_bytes)?;
        let elf_file = ElfFile::new(byte_slice)?; // returns Err(&str) if ELF parse fails

        // check that elf_file is a relocatable type 
        use xmas_elf::header::Type;
        let typ = elf_file.header.pt2.type_().as_type();
        if typ != Type::Relocatable {
            error!("load_crate_sections(): crate \"{}\" was a {:?} Elf File, must be Relocatable!", crate_name, typ);
            return Err("not a relocatable elf file");
        }

        debug!("Parsing Elf kernel crate: {:?}, size {:#x}({})", crate_name, size_in_bytes, size_in_bytes);

        // allocate enough space to load the sections
        let section_pages = allocate_section_pages(&elf_file, kernel_mmi)?;
        let text_pages    = section_pages.text_pages  .map(|tp| Arc::new(Mutex::new(tp)));
        let rodata_pages  = section_pages.rodata_pages.map(|rp| Arc::new(Mutex::new(rp)));
        let data_pages    = section_pages.data_pages  .map(|dp| Arc::new(Mutex::new(dp)));

        let mut text_pages_locked   = text_pages  .as_ref().map(|tp| tp.lock());
        let mut rodata_pages_locked = rodata_pages.as_ref().map(|rp| rp.lock());
        let mut data_pages_locked   = data_pages  .as_ref().map(|dp| dp.lock());

        // iterate through the symbol table so we can find which sections are global (publicly visible)
        // we keep track of them here in a list
        let global_sections: BTreeSet<usize> = {
            // For us to properly load the ELF file, it must NOT have been stripped,
            // meaning that it must still have its symbol table section. Otherwise, relocations will not work.
            let symtab = find_symbol_table(&elf_file)?;

            let mut globals: BTreeSet<usize> = BTreeSet::new();
            use xmas_elf::symbol_table::Entry;
            for entry in symtab.iter() {
                // Previously we were ignoring "GLOBAL" symbols with "HIDDEN" visibility, but that excluded some symbols. 
                // So now, we include all symbols with "GLOBAL" binding, regardless of visibility.  
                if entry.get_binding() == Ok(xmas_elf::symbol_table::Binding::Global) {
                    if let Ok(typ) = entry.get_type() {
                        if typ == xmas_elf::symbol_table::Type::Func || typ == xmas_elf::symbol_table::Type::Object {
                            globals.insert(entry.shndx() as usize);
                        }
                    }
                }
            }
            globals 
        };

        // this maps section header index (shndx) to LoadedSection
        let mut loaded_sections: BTreeMap<usize, StrongSectionRef> = BTreeMap::new(); 

        let mut text_offset:   usize = 0;
        let mut rodata_offset: usize = 0;
        let mut data_offset:   usize = 0;
                    
        const TEXT_PREFIX:   &'static str = ".text.";
        const RODATA_PREFIX: &'static str = ".rodata.";
        const DATA_PREFIX:   &'static str = ".data.";
        const BSS_PREFIX:    &'static str = ".bss.";
        const RELRO_PREFIX:  &'static str = "rel.ro.";


        let new_crate = CowArc::new(LoadedCrate {
            crate_name:   String::from(crate_name),
            sections:     BTreeMap::new(),
            text_pages:   text_pages  .as_ref().map(|r| Arc::clone(r)),
            rodata_pages: rodata_pages.as_ref().map(|r| Arc::clone(r)),
            data_pages:   data_pages  .as_ref().map(|r| Arc::clone(r)),
        });
        let new_crate_weak_ref = CowArc::downgrade(&new_crate);

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
                        if sec_flags & (SHF_ALLOC | SHF_WRITE | SHF_EXECINSTR) != (SHF_ALLOC | SHF_EXECINSTR) {
                            error!(".text section [{}], name: {:?} had the wrong flags {:#X}", shndx, name, sec_flags);
                            return Err(".text section had wrong flags!");
                        }

                        if let (Some(ref tp_ref), Some(ref mut tp)) = (&text_pages, &mut text_pages_locked) {
                            // here: we're ready to copy the text section to the proper address
                            let dest_addr = tp.address_at_offset(text_offset)
                                .ok_or_else(|| "BUG: text_offset wasn't within text_mapped_pages")?;
                            let dest_slice: &mut [u8]  = try!(tp.as_slice_mut(text_offset, sec_size));
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
                                    Arc::clone(tp_ref),
                                    text_offset,
                                    dest_addr,
                                    sec_size,
                                    global_sections.contains(&shndx),
                                    new_crate_weak_ref.clone(),
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
                        if sec_flags & (SHF_ALLOC | SHF_WRITE | SHF_EXECINSTR) != (SHF_ALLOC) {
                            error!(".rodata section [{}], name: {:?} had the wrong flags {:#X}", shndx, name, sec_flags);
                            return Err(".rodata section had wrong flags!");
                        }

                        if let (Some(ref rp_ref), Some(ref mut rp)) = (&rodata_pages, &mut rodata_pages_locked) {
                            // here: we're ready to copy the rodata section to the proper address
                            let dest_addr = rp.address_at_offset(rodata_offset)
                                .ok_or_else(|| "BUG: rodata_offset wasn't within rodata_mapped_pages")?;
                            let dest_slice: &mut [u8]  = try!(rp.as_slice_mut(rodata_offset, sec_size));
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
                                    Arc::clone(rp_ref),
                                    rodata_offset,
                                    dest_addr,
                                    sec_size,
                                    global_sections.contains(&shndx),
                                    new_crate_weak_ref.clone(),
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
                        if sec_flags & (SHF_ALLOC | SHF_WRITE | SHF_EXECINSTR) != (SHF_ALLOC | SHF_WRITE) {
                            error!(".data section [{}], name: {:?} had the wrong flags {:#X}", shndx, name, sec_flags);
                            return Err(".data section had wrong flags!");
                        }
                        
                        if let (Some(ref dp_ref), Some(ref mut dp)) = (&data_pages, &mut data_pages_locked) {
                            // here: we're ready to copy the data/bss section to the proper address
                            let dest_addr = dp.address_at_offset(data_offset)
                                .ok_or_else(|| "BUG: data_offset wasn't within data_pages")?;
                            let dest_slice: &mut [u8]  = try!(dp.as_slice_mut(data_offset, sec_size));
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
                                    Arc::clone(dp_ref),
                                    data_offset,
                                    dest_addr,
                                    sec_size,
                                    global_sections.contains(&shndx),
                                    new_crate_weak_ref.clone(),
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
                        if sec_flags & (SHF_ALLOC | SHF_WRITE | SHF_EXECINSTR) != (SHF_ALLOC | SHF_WRITE) {
                            error!(".bss section [{}], name: {:?} had the wrong flags {:#X}", shndx, name, sec_flags);
                            return Err(".bss section had wrong flags!");
                        }
                        
                        // we still use DataSection to represent the .bss sections, since they have the same flags
                        if let (Some(ref dp_ref), Some(ref mut dp)) = (&data_pages, &mut data_pages_locked) {
                            // here: we're ready to fill the bss section with zeroes at the proper address
                            let dest_addr = dp.address_at_offset(data_offset)
                                .ok_or_else(|| "BUG: data_offset wasn't within data_pages")?;
                            let dest_slice: &mut [u8]  = try!(dp.as_slice_mut(data_offset, sec_size));
                            for b in dest_slice {
                                *b = 0;
                            };

                            loaded_sections.insert(shndx, 
                                Arc::new(Mutex::new(LoadedSection::new(
                                    SectionType::Bss,
                                    demangled.no_hash,
                                    demangled.hash,
                                    Arc::clone(dp_ref),
                                    data_offset,
                                    dest_addr,
                                    sec_size,
                                    global_sections.contains(&shndx),
                                    new_crate_weak_ref.clone(),
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

        // set the new_crate's sections list, since we didn't do it earlier
        {
            let mut new_crate_mut = new_crate.lock_as_mut()
                .ok_or_else(|| "BUG: load_crate_sections(): couldn't get exclusive mutable access to new_crate")?;
            new_crate_mut.sections = loaded_sections;
        }

        Ok((new_crate, elf_file))
    }

        
    /// The second stage of parsing and loading a new kernel crate, 
    /// filling in the missing relocation information in the already-loaded sections. 
    /// It also remaps the `new_crate`'s MappedPages according to each of their section permissions.
    fn perform_relocations(
        &self,
        elf_file: &ElfFile,
        new_crate_ref: &StrongCrateRef,
        backup_namespace: Option<&CrateNamespace>,
        kernel_mmi: &mut MemoryManagementInfo,
        verbose_log: bool
    ) -> Result<(), &'static str> {
        let new_crate = new_crate_ref.lock_as_ref();
        if verbose_log { debug!("=========== moving on to the relocations for crate {} =========", new_crate.crate_name); }
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
                if  name.starts_with(".rela.eh_frame") || 
                    name.starts_with(".rela.note")     ||   // ignore GNU note sections
                    name.starts_with(".rela.gcc")      ||   // ignore gcc special sections for now
                    name.starts_with(".rela.debug")         // ignore debug special sections for now
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
            let target_sec_shndx = sec.info() as usize;
            let target_sec_ref = new_crate.sections.get(&target_sec_shndx).ok_or_else(|| {
                error!("ELF file error: target section was not loaded for Rela section {:?}!", sec.get_name(&elf_file));
                "target section was not loaded for Rela section"
            })?; 
            
            let mut target_sec_dependencies: Vec<StrongDependency> = Vec::new();
            let mut target_sec_internal_dependencies: Vec<InternalDependency> = Vec::new();

            let mut target_sec = target_sec_ref.lock();
            {
                let mut target_sec_mapped_pages = target_sec.mapped_pages.lock();

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
                    
                    let mut source_and_target_in_same_crate = false;

                    // We first try to get the source section from loaded_sections, which works if the section is in the crate currently being loaded.
                    let source_sec_ref = match new_crate.sections.get(&source_sec_shndx) {
                        Some(ss) => {
                            source_and_target_in_same_crate = true;
                            Ok(ss.clone())
                        }

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
                                self.get_symbol_or_load(&demangled.no_hash, CrateType::Kernel.prefix(), backup_namespace, kernel_mmi, verbose_log)
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

                    let source_and_target_are_same_section = Arc::ptr_eq(&source_sec_ref, &target_sec_ref);
                    let relocation_entry = RelocationEntry::from_elf_relocation(rela_entry);

                    write_relocation(
                        relocation_entry,
                        &mut target_sec_mapped_pages,
                        target_sec.mapped_pages_offset,
                        if source_and_target_are_same_section { target_sec.virt_addr() } else { source_sec_ref.lock().virt_addr() },
                        verbose_log
                    )?;

                    if source_and_target_in_same_crate {
                        // We keep track of relocation information so that we can be aware of and faithfully reconstruct 
                        // inter-section dependencies even within the same crate.
                        // This is necessary for doing a deep copy of the crate in memory, 
                        // without having to re-parse that crate's ELF file (and requiring the ELF file to still exist)
                        target_sec_internal_dependencies.push(InternalDependency::new(relocation_entry, source_sec_shndx))
                    }
                    else {
                        // tell the source_sec that the target_sec is dependent upon it
                        let weak_dep = WeakDependent {
                            section: Arc::downgrade(&target_sec_ref),
                            relocation: relocation_entry,
                        };
                        source_sec_ref.lock().sections_dependent_on_me.push(weak_dep);
                        
                        // tell the target_sec that it has a strong dependency on the source_sec
                        let strong_dep = StrongDependency {
                            section: Arc::clone(&source_sec_ref),
                            relocation: relocation_entry,
                        };
                        target_sec_dependencies.push(strong_dep);          
                    }
                }
            }

            // add the target section's dependencies and relocation details all at once
            target_sec.sections_i_depend_on.append(&mut target_sec_dependencies);
            target_sec.internal_dependencies.append(&mut target_sec_internal_dependencies);
        }
        // here, we're done with handling all the relocations in this entire crate


        // Finally, remap each section's mapped pages to the proper permission bits, 
        // since we initially mapped them all as writable
        if let PageTable::Active(ref mut active_table) = kernel_mmi.page_table {
            if let Some(ref tp) = new_crate.text_pages { 
                tp.lock().remap(active_table, TEXT_SECTION_FLAGS())?;
            }
            if let Some(ref rp) = new_crate.rodata_pages {
                rp.lock().remap(active_table, RODATA_SECTION_FLAGS())?;
            }
            // data/bss sections are already mapped properly, since they're supposed to be writable
        }
        else {
            return Err("couldn't get kernel's active page table");
        }

        Ok(())
    }

    
    /// Adds the given symbol to this namespace's symbol map.
    /// If the symbol already exists in the symbol map, this leaves the existing symbol intact and *does not* replace it.
    /// Returns true if the symbol was added, and false if it already existed and thus was not added.
    fn add_symbol(
        existing_symbol_map: &mut SymbolMap,
        new_section_key: String,
        new_section_ref: &StrongSectionRef
    ) -> bool {
        match existing_symbol_map.entry(new_section_key) {
            Entry::Occupied(_old_val) => {
                false
                // if log_replacements {
                //     if sec.name.ends_with("_LOC") || crate_name == "nano_core" {
                //         // ignoring these special cases currently
                //     }
                //     else {
                //         if let Some(old_sec) = _old_val.get().upgrade() {
                //             let new_sec_size = sec.size;
                //             let old_sec_size = old_sec.lock().size;
                //             if old_sec_size == new_sec_size {
                //                 info!("       add_symbols \"{}\": Ignoring new symbol already present: {}", crate_name, sec.name);
                //             }
                //             else {
                //                 warn!("       add_symbols \"{}\": unexpected: different section sizes (old={}, new={}), ignoring new symbol: {}", 
                //                     crate_name, old_sec_size, new_sec_size, sec.name);
                //             }
                //         }
                //     }
                // }
            }
            Entry::Vacant(new_entry) => {
                new_entry.insert(Arc::downgrade(new_section_ref));
                true
            }
        }
    }

    /// Adds all global symbols in the given `sections` iterator to this namespace's symbol map. 
    /// 
    /// Returns the number of *new* unique symbols added.
    /// 
    /// # Note
    /// If a symbol already exists in the symbol map, this leaves the existing symbol intact and *does not* replace it.
    pub fn add_symbols<'a, I>(
        &self, 
        sections: I,
        _log_replacements: bool,
    ) -> usize
        where I: IntoIterator<Item = &'a StrongSectionRef>,
    {
        self.add_symbols_filtered(sections, |_sec| true, _log_replacements)
    }


    /// Adds symbols in the given `sections` iterator to this namespace's symbol map,
    /// but only the symbols that correspond to *global* sections AND for which the given `filter_func` returns true. 
    /// 
    /// Returns the number of *new* unique symbols added.
    /// 
    /// # Note
    /// If a symbol already exists in the symbol map, this leaves the existing symbol intact and *does not* replace it.
    fn add_symbols_filtered<'a, I, F>(
        &self, 
        sections: I,
        filter_func: F,
        _log_replacements: bool,
    ) -> usize
        where I: IntoIterator<Item = &'a StrongSectionRef>,
              F: Fn(&LoadedSection) -> bool
    {
        let mut existing_map = self.symbol_map.lock();

        // add all the global symbols to the symbol map, in a way that lets us inspect/log each one
        let mut count = 0;
        for sec_ref in sections.into_iter() {
            let sec = sec_ref.lock();
            
            if filter_func(&sec) && sec.global {
                let added = CrateNamespace::add_symbol(&mut existing_map, sec.name.clone(), sec_ref);
                if added {
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
    /// If the symbol cannot be found in this namespace, it does the following:    
    /// (1) First, try to find the missing symbol in the `backup_namespace`. 
    ///     If we find it there, then add that shared crate into this namespace,
    ///     and add all of that shared crate's symbols into this crate as well. 
    /// 
    /// (2) Second, if the missing symbol isn't in the backup namespace either, 
    ///     try to load its containing crate from the module file. 
    ///     This can only be done for symbols that have a leading crate name, such as "my_crate::foo";
    ///     if a symbol was given the `no_mangle` attribute, then we will not be able to find it
    ///     and that symbol's containing crate should be manually loaded before invoking this. 
    /// 
    /// # Arguments
    /// * `demangled_full_symbol`: a fully-qualified symbol string, e.g., "my_crate::MyStruct::do_foo".
    /// * `kernel_crate_prefix`: the prefix string that goes in front of crate module names, 
    ///   which is generally `"k#"`. 
    ///   You can specify the default by passing in `CrateType::Kernel.prefix()`, or specify another prefix
    ///   to help the symbol resolver know in which crate modules to look.
    /// * `backup_namespace`: the `CrateNamespace` that should be searched for missing symbols 
    ///   if a symbol cannot be found in this `CrateNamespace`. 
    ///   For example, the default namespace could be used by passing in `Some(get_default_namespace())`.
    ///   If `backup_namespace` is `None`, then no other namespace will be searched.
    /// * `kernel_mmi`: a mutable reference to the kernel's `MemoryManagementInfo`.
    pub fn get_symbol_or_load(
        &self, 
        demangled_full_symbol: &str, 
        kernel_crate_prefix: &str,
        backup_namespace: Option<&CrateNamespace>, 
        kernel_mmi: &mut MemoryManagementInfo,
        verbose_log: bool
    ) -> WeakSectionRef {
        // First, see if the section for the given symbol is already available and loaded
        if let Some(sec) = self.get_symbol_internal(demangled_full_symbol) {
            return sec;
        }


        // If not, our second try is to check the backup_namespace
        // to see if that namespace already has the section we want
        if let Some(backup) = backup_namespace {
            info!("Symbol \"{}\" not initially found, attemping to load it from backup namespace {:?}", 
                demangled_full_symbol, backup.name);
            if let Some(weak_sec) = backup.get_symbol_internal(demangled_full_symbol) {
                if let Some(sec) = weak_sec.upgrade() {
                    // If we found it in the backup_namespace, then that saves us the effort of having to load the crate again.
                    // We need to add a shared reference to that section's parent crate to this namespace as well, 
                    // so it can't be dropped while this namespace is still relying on it.  
                    let pcref_opt = { sec.lock().parent_crate.upgrade() };
                    if let Some(parent_crate_ref) = pcref_opt {
                        let parent_crate_name = {
                            let parent_crate = parent_crate_ref.lock_as_ref();
                            // (1) We could either add just this one missing symbol ...
                            // self.add_symbols(Some(sec.clone()).iter(), true);
                            // (2) Or add all symbols from the already-loaded crate in the backup namespace
                            self.add_symbols(parent_crate.sections.values(), true);
                            parent_crate.crate_name.clone()
                        };
                        // info!("Using symbol {:?} (crate {:?}) from backup namespace {:?} in new namespace {:?}",
                        //     demangled_full_symbol, parent_crate_name, backup.name, self.name);
                        self.crate_tree.lock().insert(parent_crate_name, parent_crate_ref.clone());
                        return weak_sec;
                    }
                    else {
                        error!("get_symbol_or_load(): found symbol \"{}\" in backup namespace, but unexpectedly couldn't get its section's parent crate!",
                            demangled_full_symbol);
                        return Weak::default();
                    }
                }
            }
        }


        // If we couldn't get the symbol, then we attempt to load the kernel crate containing that symbol.
        // We are only able to do this for mangled symbols, those that have a leading crate name,
        // such as "my_crate::foo". 
        // If "foo()" was marked no_mangle, then we don't know which crate to load because there is no "my_crate::" before it.
        if let Some(crate_dependency_name) = get_containing_crate_name(demangled_full_symbol) {
            info!("Symbol \"{}\" not initially found, attemping to load its containing crate {:?}", 
                demangled_full_symbol, crate_dependency_name);
            
            // module names have a prefix like "k#", so we need to prepend that to the crate name
            let crate_dependency_name = format!("{}{}", kernel_crate_prefix, crate_dependency_name);
 
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
                else {
                    error!("Found symbol's (\"{}\") containing crate, but load_kernel_crate failed for that crate module {:?}.",
                        demangled_full_symbol, dependency_module);
                }
            }
            else {
                error!("Symbol \"{}\" not found, and couldn't find its containing crate's module \"{}\".", 
                    demangled_full_symbol, crate_dependency_name);
            }
        }


        error!("Symbol \"{}\" not found, cannot determine its containing crate (no leading crate name). Try loading the crate manually first.", 
            demangled_full_symbol);    
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
fn get_containing_crate_name<'a>(demangled_full_symbol: &'a str) -> Option<&'a str> {
    demangled_full_symbol.split("::").next().and_then(|s| {
        // Get the last word right before the first "::"
        s.rsplit(|c| !is_valid_crate_name_char(c))
            .next() // the first element of the iterator (last element before the "::")
    })
}


/// Similar to [`get_containing_crate_name()`](#method.get_containing_crate_name),
/// but replaces the parent crate name with the given `new_crate_name`, 
/// if it can be found, and if the parent crate name matches the `old_crate_name`. 
/// If the parent crate name can be found but does not match the expected `old_crate_name`,
/// then None is returned.
/// 
/// This creates an entirely new String rather than performing an in-place replacement, 
/// because the `new_crate_name` might be a different length than the original crate name.
/// # Examples
/// `keyboard::init  -->  keyboard_new::init`
fn replace_containing_crate_name<'a>(demangled_full_symbol: &'a str, old_crate_name: &str, new_crate_name: &str) -> Option<String> {
    // debug!("replace_containing_crate_name(dfs: {:?}, old: {:?}, new: {:?})", demangled_full_symbol, old_crate_name, new_crate_name);
    demangled_full_symbol.match_indices("::").next().and_then(|(index_of_first_double_colon, _substr)| {
        // Get the last word right before the first "::"
        demangled_full_symbol.get(.. index_of_first_double_colon).and_then(|substr| {
            // debug!("  replace_containing_crate_name(\"{}\"): substr: \"{}\" at index {}", demangled_full_symbol, substr, index_of_first_double_colon);
            let start_idx = substr.rmatch_indices(|c| !is_valid_crate_name_char(c))
                .next() // the first element right before the crate name starts
                .map(|(index_right_before_parent_crate_name, _substr)| { index_right_before_parent_crate_name + 1 }) // advance to the actual start 
                .unwrap_or(0); // if we didn't find any match, it means that everything since the beginning of the string was a valid char part of the parent_crate_name 

            demangled_full_symbol.get(start_idx .. index_of_first_double_colon)
                .filter(|&parent_crate_name| { parent_crate_name == old_crate_name })
                .and_then(|_parent_crate_name| {
                    // debug!("    replace_containing_crate_name(\"{}\"): parent_crate_name: \"{}\" at index [{}-{})", demangled_full_symbol, _parent_crate_name, start_idx, index_of_first_double_colon);
                    demangled_full_symbol.get(.. start_idx).and_then(|before_crate_name| {
                        demangled_full_symbol.get(index_of_first_double_colon ..).map(|after_crate_name| {
                            format!("{}{}{}", before_crate_name, new_crate_name, after_crate_name)
                        })
                    })
            })
        })
    })
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

        let mut allocate_pages_closure = |size_in_bytes: usize, flags: EntryFlags| {
            let allocated_pages = try!(allocate_pages_by_bytes(size_in_bytes).ok_or("Couldn't allocate_pages_by_bytes, out of virtual address space"));

            if let PageTable::Active(ref mut active_table) = kernel_mmi.page_table {
                // Right now we're just simply copying small sections to the new memory,
                // so we have to map those pages to real (randomly chosen) frames first. 
                // because we're copying bytes to the newly allocated pages, we need to make them writable too, 
                // and then change the page permissions (by using remap) later. 
                active_table.map_allocated_pages(allocated_pages, flags | EntryFlags::PRESENT | EntryFlags::WRITABLE, frame_allocator.deref_mut())
            }
            else {
                return Err("couldn't get kernel's active page table");
            }
    
        };

        // we must allocate these pages separately because they will have different flags later
        (
            if text_bytecount   > 0 { allocate_pages_closure(text_bytecount,   TEXT_SECTION_FLAGS()).ok()     } else { None }, 
            if rodata_bytecount > 0 { allocate_pages_closure(rodata_bytecount, RODATA_SECTION_FLAGS()).ok()   } else { None }, 
            if data_bytecount   > 0 { allocate_pages_closure(data_bytecount,   DATA_BSS_SECTION_FLAGS()).ok() } else { None }
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
/// * `target_sec_mapped_pages`: the `MappedPages` that covers the target section, i.e., the section where the relocation data will be written to.
/// * `target_sec_mapped_pages_offset`: the offset into `target_sec_mapped_pages` where the target section is located.
/// * `source_sec`: the source section of the relocation, i.e., the section that the `target_sec` depends on and "points" to.
/// * `verbose_log`: whether to output verbose logging information about this relocation action.
fn write_relocation(
    relocation_entry: RelocationEntry,
    target_sec_mapped_pages: &mut MappedPages,
    target_sec_mapped_pages_offset: usize,
    source_sec_vaddr: VirtualAddress,
    verbose_log: bool
) -> Result<(), &'static str>
{
    // Calculate exactly where we should write the relocation data to.
    let target_offset = target_sec_mapped_pages_offset + relocation_entry.offset;

    // Perform the actual relocation data writing here.
    // There is a great, succint table of relocation types here
    // https://docs.rs/goblin/0.0.13/goblin/elf/reloc/index.html
    match relocation_entry.typ {
        R_X86_64_32 => {
            let target_ref: &mut u32 = try!(target_sec_mapped_pages.as_type_mut(target_offset));
            let source_val = source_sec_vaddr.wrapping_add(relocation_entry.addend);
            if verbose_log { trace!("                    target_ptr: {:#X}, source_val: {:#X} (from sec_vaddr {:#X})", target_ref as *mut _ as usize, source_val, source_sec_vaddr); }
            *target_ref = source_val as u32;
        }
        R_X86_64_64 => {
            let target_ref: &mut u64 = try!(target_sec_mapped_pages.as_type_mut(target_offset));
            let source_val = source_sec_vaddr.wrapping_add(relocation_entry.addend);
            if verbose_log { trace!("                    target_ptr: {:#X}, source_val: {:#X} (from sec_vaddr {:#X})", target_ref as *mut _ as usize, source_val, source_sec_vaddr); }
            *target_ref = source_val as u64;
        }
        R_X86_64_PC32 => {
            let target_ref: &mut u32 = try!(target_sec_mapped_pages.as_type_mut(target_offset));
            let source_val = source_sec_vaddr.wrapping_add(relocation_entry.addend).wrapping_sub(target_ref as *mut _ as usize);
            if verbose_log { trace!("                    target_ptr: {:#X}, source_val: {:#X} (from sec_vaddr {:#X})", target_ref as *mut _ as usize, source_val, source_sec_vaddr); }
            *target_ref = source_val as u32;
        }
        R_X86_64_PC64 => {
            let target_ref: &mut u64 = try!(target_sec_mapped_pages.as_type_mut(target_offset));
            let source_val = source_sec_vaddr.wrapping_add(relocation_entry.addend).wrapping_sub(target_ref as *mut _ as usize);
            if verbose_log { trace!("                    target_ptr: {:#X}, source_val: {:#X} (from sec_vaddr {:#X})", target_ref as *mut _ as usize, source_val, source_sec_vaddr); }
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

    Ok(())
}


#[allow(dead_code)]
fn dump_dependent_crates(krate: &LoadedCrate, prefix: String) {
	for weak_crate_ref in krate.crates_dependent_on_me() {
		let strong_crate_ref = weak_crate_ref.upgrade().unwrap();
        let strong_crate = strong_crate_ref.lock_as_ref();
		debug!("{}{}", prefix, strong_crate.crate_name);
		dump_dependent_crates(&*strong_crate, format!("{}  ", prefix));
	}
}


#[allow(dead_code)]
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
