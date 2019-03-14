//! This metadata module contains metadata about all other modules/crates loaded in Theseus.
//! 
//! [This is a good link](https://users.rust-lang.org/t/circular-reference-issue/9097)
//! for understanding why we need `Arc`/`Weak` to handle recursive/circular data structures in Rust. 

use core::ops::{Deref};
use spin::Mutex;
use alloc::vec::Vec;
use alloc::string::String;
use alloc::collections::{BTreeMap, BTreeSet};
use alloc::sync::{Arc, Weak};
use memory::{MappedPages, VirtualAddress, PageTable, MemoryManagementInfo, EntryFlags, FrameAllocator};
use dependency::*;
use super::{TEXT_SECTION_FLAGS, RODATA_SECTION_FLAGS, DATA_BSS_SECTION_FLAGS};
use cow_arc::{CowArc, CowWeak};
use path::Path;

use super::SymbolMap;
use qp_trie::{Trie, wrapper::BString};

/// A Strong reference (`Arc`) to a `LoadedSection`.
pub type StrongSectionRef  = Arc<Mutex<LoadedSection>>;
/// A Weak reference (`Weak`) to a `LoadedSection`.
pub type WeakSectionRef = Weak<Mutex<LoadedSection>>;

/// A Strong reference (`Arc`) to a `LoadedCrate`.
pub type StrongCrateRef  = CowArc<LoadedCrate>; // Arc<Mutex<LoadedCrate>>;
/// A Weak reference (`Weak`) to a `LoadedCrate`.
pub type WeakCrateRef = CowWeak<LoadedCrate>; // Weak<Mutex<LoadedCrate>>;


const CRATE_PREFIX_DELIMITER: &'static str = "#";

#[derive(Debug, PartialEq)]
pub enum CrateType {
    Kernel,
    Application,
    Userspace,
}
impl CrateType {
    fn first_char(&self) -> &'static str {
        match self {
            CrateType::Kernel       => "k",
            CrateType::Application  => "a",
            CrateType::Userspace    => "u",
        }
    }

    /// Returns a tuple of (CrateType, &str, &str) based on the given `module_name`,
    /// in which the `CrateType` is based on the first character, 
    /// the first `&str` is the namespace prefix, e.g., `"sse"` in `"k_sse#..."`,
    /// and the second `&str` is the rest of the module file name after the prefix delimiter `"#"`.
    /// 
    /// # Examples 
    /// ```
    /// let result = CrateType::from_module_name("k#my_crate.o");
    /// assert_eq!(result, (CrateType::Kernel, "", "my_crate.o") );
    /// 
    /// let result = CrateType::from_module_name("ksse#my_crate.o");
    /// assert_eq!(result, (CrateType::Kernel, "sse", "my_crate") );
    /// ```
    pub fn from_module_name<'a>(module_name: &'a str) -> Result<(CrateType, &'a str, &'a str), &'static str> {
        let mut iter = module_name.split(CRATE_PREFIX_DELIMITER);
        let prefix = iter.next().ok_or("couldn't parse crate type prefix before delimiter")?;
        let crate_name = iter.next().ok_or("couldn't parse crate name after prefix delimiter")?;
        if iter.next().is_some() {
            return Err("found more than one '#' delimiter in module name");
        }
        let namespace_prefix = prefix.get(1..).unwrap_or("");
        
        if prefix.starts_with(CrateType::Kernel.first_char()) {
            Ok((CrateType::Kernel, namespace_prefix, crate_name))
        }
        else if prefix.starts_with(CrateType::Application.first_char()) {
            Ok((CrateType::Application, namespace_prefix, crate_name))
        }
        else if prefix.starts_with(CrateType::Userspace.first_char()) {
            Ok((CrateType::Userspace, namespace_prefix, crate_name))
        }
        else {
            Err("module_name didn't start with a known CrateType prefix")
        }
    }


    pub fn is_application(module_name: &str) -> bool {
        module_name.starts_with(CrateType::Application.first_char())
    }

    pub fn is_kernel(module_name: &str) -> bool {
        module_name.starts_with(CrateType::Kernel.first_char())
    }

    pub fn is_userspace(module_name: &str) -> bool {
        module_name.starts_with(CrateType::Userspace.first_char())
    }
}


/// Represents a single crate object file that has been loaded into the system.
pub struct LoadedCrate {
    /// The name of this crate.
    pub crate_name: String,
    /// The absolute path of the object file that this crate was loaded from.
    pub object_file_abs_path: Path,
    /// A map containing all the sections in this crate.
    /// In general we're only interested the values (the `LoadedSection`s themselves),
    /// but we keep each section's shndx (section header index from its crate's ELF file)
    /// as the key because it helps us quickly handle relocations and crate swapping.
    pub sections: BTreeMap<usize, StrongSectionRef>,
    /// The `MappedPages` that include the text sections for this crate,
    /// i.e., sections that are readable and executable, but not writable.
    pub text_pages: Option<Arc<Mutex<MappedPages>>>,
    /// The `MappedPages` that include the rodata sections for this crate.
    /// i.e., sections that are read-only, not writable nor executable.
    pub rodata_pages: Option<Arc<Mutex<MappedPages>>>,
    /// The `MappedPages` that include the data and bss sections for this crate.
    /// i.e., sections that are readable and writable but not executable.
    pub data_pages: Option<Arc<Mutex<MappedPages>>>,
    
    // The members below are most used to accelerate crate swapping //

    /// The set of global symbols in this crate, including regular ones 
    /// that are prefixed with the `crate_name` and `no_mangle` symbols that are not.
    pub global_symbols: BTreeSet<BString>,
    /// The set of BSS sections in this crate.
    /// The key is the section name and the value is a reference to the section;
    /// these sections are also in the `sections` member above.
    pub bss_sections: Trie<BString, StrongSectionRef>,
    /// The set of symbols that this crate's global symbols are reexported under,
    /// i.e., they have been added to the enclosing `CrateNamespace`'s symbol map under these names.
    /// 
    /// This is primarily used when swapping crates, and it is useful in the following way. 
    /// If this crate is the new crate that is swapped in to replace another crate, 
    /// and the caller of the `swap_crates()` function specifies that this crate 
    /// should expose its symbols with names that match the old crate it's replacing, 
    /// then this will be populated with the names of corresponding symbols from the old crate that its replacing.
    /// For example, if this crate has a symbol `keyboard::init::h456`, and it replaced an older crate
    /// that had the symbol `keyboard::init::123`, and `reexport_new_symbols_as_old` was true,
    /// then `keyboard::init::h123` will be added to this set.
    /// 
    /// When a crate is first loaded, this will be empty by default, 
    /// because this crate will only have populated its `global_symbols` set during loading. 
    pub reexported_symbols: BTreeSet<BString>,
}

use core::fmt;
impl fmt::Debug for LoadedCrate {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "LoadedCrate(name: {:?}, objfile: {:?})", 
            self.crate_name, 
            self.object_file_abs_path,
        )
    }
}

impl Drop for LoadedCrate {
    fn drop(&mut self) {
        trace!("### Dropped LoadedCrate: {}", self.crate_name);
    }
}

impl LoadedCrate {
    /// Returns the `LoadedSection` of type `SectionType::Text` that matches the requested function name, if it exists in this `LoadedCrate`.
    /// Only matches demangled names, e.g., "my_crate::foo".
    pub fn get_function_section(&self, func_name: &str) -> Option<&StrongSectionRef> {
        self.find_section(|sec| sec.is_text() && sec.name == func_name)
    }

    /// Returns the first `LoadedSection` that matches the given predicate
    pub fn find_section<F>(&self, predicate: F) -> Option<&StrongSectionRef> 
        where F: Fn(&LoadedSection) -> bool
    {
        self.sections.values()
            .filter(|sec_ref| predicate(&sec_ref.lock()))
            .next()
    }

    /// Returns the substring of this crate's name that excludes the trailing hash. 
    /// If there is no hash, then it returns the entire name. 
    pub fn crate_name_without_hash(&self) -> &str {
        // the hash identifier (delimiter) is "-"
        self.crate_name.split("-")
            .next()
            .unwrap_or_else(|| &self.crate_name)
    }

    /// Returns this crate name as a symbol prefix, including a trailing "`::`".
    /// If there is no hash, then it returns the entire name with a trailing "`::`".
    /// # Example
    /// * Crate name: "`device_manager-e3769b63863a4030`", return value: "`device_manager::`"
    /// * Crate name: "`hello`"` return value: "`hello::`"
    pub fn crate_name_as_prefix(&self) -> String {
        format!("{}::", self.crate_name_without_hash())
    }

    /// Currently may contain duplicates!
    pub fn crates_dependent_on_me(&self) -> Vec<WeakCrateRef> {
        let mut results: Vec<WeakCrateRef> = Vec::new();
        for sec in self.sections.values() {
            let sec_locked = sec.lock();
            for dep_sec in &sec_locked.sections_dependent_on_me {
                if let Some(dep_sec) = dep_sec.section.upgrade() {
                    let dep_sec_locked = dep_sec.lock();
                    let parent_crate = dep_sec_locked.parent_crate.clone();
                    results.push(parent_crate);
                }
            }
        }
        results
    }

    /// Creates a new copy of this `LoadedCrate`, which is a relatively slow process
    /// because it must do the following:    
    /// * Deep copy all of the MappedPages into completely new memory regions.
    /// * Duplicate every section within this crate.
    /// * Recalculate every relocation entry to point to the newly-copied sections,
    ///   which is the most time-consuming component of this function.
    /// 
    /// # Notes
    /// This is obviously different from cloning a shared Arc reference to this `LoadedCrate`,
    /// i.e., a `StrongCrateRef`, which is an instant and cheap operation that does not duplicate the underlying `LoadedCrate`.
    /// 
    /// Also, there is currently no way to deep copy a single `LoadedSection` in isolation,
    /// because a single section has dependencies on many other sections, i.e., due to relocations,
    /// and that would result in weird inconsistencies that violate those dependencies.
    /// In addition, multiple `LoadedSection`s share a given `MappedPages` memory range,
    /// so they all have to be duplicated at once into a new `MappedPages` range at the crate level.
    pub fn deep_copy<A: FrameAllocator>(
        &self, 
        kernel_mmi: &mut MemoryManagementInfo, 
        allocator: &mut A
    ) -> Result<StrongCrateRef, &'static str> {
        // First, deep copy all of the memory regions.
        // We initially map the as writable because we'll have to copy things into them
        let (new_text_pages, new_rodata_pages, new_data_pages) = {
            if let PageTable::Active(ref mut active_table) = kernel_mmi.page_table {
                let new_text_pages = match self.text_pages {
                    Some(ref tp) => Some(tp.lock().deep_copy(Some(TEXT_SECTION_FLAGS() | EntryFlags::WRITABLE), active_table, allocator)?),
                    None => None,
                };
                let new_rodata_pages = match self.rodata_pages {
                    Some(ref rp) => Some(rp.lock().deep_copy(Some(RODATA_SECTION_FLAGS() | EntryFlags::WRITABLE), active_table, allocator)?),
                    None => None,
                };
                let new_data_pages = match self.data_pages {
                    Some(ref dp) => Some(dp.lock().deep_copy(Some(DATA_BSS_SECTION_FLAGS()), active_table, allocator)?),
                    None => None,
                };
                (new_text_pages, new_rodata_pages, new_data_pages)
            }
            else {
                return Err("couldn't get kernel's active page table");
            }
        };

        let new_text_pages_ref   = new_text_pages  .map(|mp| Arc::new(Mutex::new(mp)));
        let new_rodata_pages_ref = new_rodata_pages.map(|mp| Arc::new(Mutex::new(mp)));
        let new_data_pages_ref   = new_data_pages  .map(|mp| Arc::new(Mutex::new(mp)));

        let mut new_text_pages_locked   = new_text_pages_ref  .as_ref().map(|tp| tp.lock());
        let mut new_rodata_pages_locked = new_rodata_pages_ref.as_ref().map(|rp| rp.lock());
        let mut new_data_pages_locked   = new_data_pages_ref  .as_ref().map(|dp| dp.lock());

        let new_crate = CowArc::new(LoadedCrate {
            crate_name:              self.crate_name.clone(),
            object_file_abs_path:    self.object_file_abs_path.clone(),
            sections:                BTreeMap::new(),
            text_pages:              new_text_pages_ref.clone(),
            rodata_pages:            new_rodata_pages_ref.clone(),
            data_pages:              new_data_pages_ref.clone(),
            global_symbols:          self.global_symbols.clone(),
            bss_sections:            Trie::new(),
            reexported_symbols:      self.reexported_symbols.clone(),
        });
        let new_crate_weak_ref = CowArc::downgrade(&new_crate);

        // Second, deep copy the entire list of sections and fix things that don't make sense to directly clone:
        // 1) The parent_crate reference itself, since we're replacing that with a new one,
        // 2) The section's mapped_pages, which will point to a new `MappedPages` object for the newly-copied crate,
        // 3) The section's virt_addr, which is based on its new mapped_pages
        let mut new_sections: BTreeMap<usize, StrongSectionRef> = BTreeMap::new();
        let mut new_bss_sections: Trie<BString, StrongSectionRef> = Trie::new();
        for (shndx, old_sec_ref) in self.sections.iter() {
            let old_sec = old_sec_ref.lock();
            let new_sec_mapped_pages_offset = old_sec.mapped_pages_offset;
            let (new_sec_mapped_pages_ref, new_sec_virt_addr) = match old_sec.typ {
                SectionType::Text => (
                    new_text_pages_ref.clone().ok_or_else(|| "BUG: missing text pages in newly-copied crate")?,
                    new_text_pages_locked.as_ref().and_then(|tp| tp.address_at_offset(new_sec_mapped_pages_offset)),
                ),
                SectionType::Rodata => (
                    new_rodata_pages_ref.clone().ok_or_else(|| "BUG: missing rodata pages in newly-copied crate")?,
                    new_rodata_pages_locked.as_ref().and_then(|rp| rp.address_at_offset(new_sec_mapped_pages_offset)),
                ),
                SectionType::Data |
                SectionType::Bss => (
                    new_data_pages_ref.clone().ok_or_else(|| "BUG: missing data pages in newly-copied crate")?,
                    new_data_pages_locked.as_ref().and_then(|dp| dp.address_at_offset(new_sec_mapped_pages_offset)),
                ),
            };
            let new_sec_virt_addr = new_sec_virt_addr.ok_or_else(|| "BUG: couldn't get virt_addr for new section")?;

            let new_sec_ref = Arc::new(Mutex::new(LoadedSection::with_dependencies(
                old_sec.typ,                            // section type is the same
                old_sec.name.clone(),                   // name is the same
                new_sec_mapped_pages_ref,               // mapped_pages is different, points to the new duplicated one
                new_sec_mapped_pages_offset,            // mapped_pages_offset is the same
                new_sec_virt_addr,                      // virt_addr is different, based on the new mapped_pages
                old_sec.size,                           // size is the same
                old_sec.global,                         // globalness is the same
                new_crate_weak_ref.clone(),             // parent_crate is different, points to the newly-copied crate
                old_sec.sections_i_depend_on.clone(),   // dependencies are the same, but relocations need to be re-written
                Vec::new(),                             // no sections can possibly depend on this one, since we just created it
                old_sec.internal_dependencies.clone()   // internal dependencies are the same, but relocations need to be re-written
            )));

            if old_sec.typ == SectionType::Bss {
                new_bss_sections.insert_str(&old_sec.name, new_sec_ref.clone());
            }
            new_sections.insert(*shndx, new_sec_ref);
        }


        // Now we can go through the list again and fix up the rest of the elements in each section.
        // The foreign sections dependencies (sections_i_depend_on) are the same, 
        // but all relocation entries must be rewritten because the sections' virtual addresses have changed.
        for new_sec_ref in new_sections.values() {
            let mut new_sec = new_sec_ref.lock();
            let new_sec_mapped_pages = match new_sec.typ {
                SectionType::Text   => new_text_pages_locked.as_mut().ok_or_else(|| "BUG: missing text pages in newly-copied crate")?,
                SectionType::Rodata => new_rodata_pages_locked.as_mut().ok_or_else(|| "BUG: missing rodata pages in newly-copied crate")?,
                SectionType::Data |
                SectionType::Bss    => new_data_pages_locked.as_mut().ok_or_else(|| "BUG: missing data pages in newly-copied crate")?,
            };
            let new_sec_mapped_pages_offset = new_sec.mapped_pages_offset;

            // The newly-duplicated crate still depends on the same sections, so we keep those as is, 
            // but we do need to recalculate those relocations.
            for mut strong_dep in new_sec.sections_i_depend_on.iter_mut() {
                // we can skip modifying "absolute" relocations, since those only depend on the source section,
                // which we haven't actually changed (we've duplicated the target section here, not the source)
                if !strong_dep.relocation.is_absolute() {
                    let mut source_sec = strong_dep.section.lock();
                    // perform the actual fix by writing the relocation
                    super::write_relocation(
                        strong_dep.relocation, 
                        new_sec_mapped_pages, 
                        new_sec_mapped_pages_offset,
                        source_sec.virt_addr,
                        true
                    )?;

                    // add this new_sec as one of the source sec's weak dependents
                    source_sec.sections_dependent_on_me.push(
                        WeakDependent {
                            section: Arc::downgrade(new_sec_ref),
                            relocation: strong_dep.relocation,
                        }
                    );
                }
            }

            // Finally, fix up all of its internal dependencies by recalculating/rewriting their relocations.
            // We shouldn't need to actually change the InternalDependency instances themselves 
            // because they are based on crate-specific section shndx values, 
            // which are completely safe to clone without needing any fix ups. 
            for internal_dep in &new_sec.internal_dependencies {
                let source_sec_ref = new_sections.get(&internal_dep.source_sec_shndx)
                    .ok_or_else(|| "Couldn't get new section specified by an internal dependency's source_sec_shndx")?;

                // The source and target (new_sec) sections might be the same, so we need to check first
                // to ensure that we don't cause deadlock by trying to lock the same section twice.
                let source_sec_vaddr = if Arc::ptr_eq(source_sec_ref, new_sec_ref) {
                    // here: the source_sec and new_sec are the same, so just use the already-locked new_sec
                    new_sec.virt_addr()
                } else {
                    // here: the source_sec and new_sec are different, so we can go ahead and safely lock the source_sec
                    source_sec_ref.lock().virt_addr()
                };
                super::write_relocation(
                    internal_dep.relocation, 
                    new_sec_mapped_pages, 
                    new_sec_mapped_pages_offset,
                    source_sec_vaddr,
                    true
                )?;
            }
        }

        // since we mapped all the new MappedPages as writable, we need to properly remap them
        if let PageTable::Active(ref mut active_table) = kernel_mmi.page_table {
            if let Some(ref mut tp) = new_text_pages_locked { 
                try!(tp.remap(active_table, TEXT_SECTION_FLAGS()));
            }
            if let Some(ref mut rp) = new_rodata_pages_locked { 
                try!(rp.remap(active_table, RODATA_SECTION_FLAGS()));
            }
            // data/bss sections are already mapped properly, since they're supposed to be writable
        }
        else {
            return Err("couldn't get kernel's active page table");
        }

        // set the new_crate's section-related lists, since we didn't do it earlier
        {
            let mut new_crate_mut = new_crate.lock_as_mut()
                .ok_or_else(|| "BUG: LoadedCrate::deep_copy(): couldn't get exclusive mutable access to copied new_crate")?;
            new_crate_mut.sections = new_sections;
            new_crate_mut.bss_sections = new_bss_sections;
        }

        Ok(new_crate)
    }
}


/// Returns a map containing all symbols,
/// filtered to include only `LoadedSection`s that satisfy the given predicate
/// (if the predicate returns true for a given section, then it is included in the map).
/// 
/// The symbols come from the sections in the given section iterator (if Some), 
/// otherwise they come from this crate's sections list.
/// 
/// See [`global_symbol_map`](#method.global_system_map) for an example.
pub fn symbol_map<'a, I, F>(
    sections: I,
    crate_name: &str,
    predicate: F,
) -> SymbolMap 
    where F: Fn(&LoadedSection) -> bool, 
            I: IntoIterator<Item = &'a StrongSectionRef> 
{
    let mut map: SymbolMap = SymbolMap::new();
    for sec in sections.into_iter().filter(|sec| predicate(sec.lock().deref())) {
        let key = sec.lock().name.clone();
        if let Some(old_val) = map.insert_str(&key, Arc::downgrade(&sec)) {
            if key.ends_with("_LOC") || crate_name == "nano_core" {
                // ignoring these special cases currently
            }
            else {
                warn!("symbol_map(): crate \"{}\" had duplicate section for symbol \"{}\", old: {:?}, new: {:?}", 
                    crate_name, key, old_val.upgrade(), sec);
            }
        }
    }

    map
}


/// The possible types of `LoadedSection`s: .text, .rodata, .data, or .bss.
/// A .bss section is basically treated the same as .data, 
/// but we keep them separate
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum SectionType {
    Text,
    Rodata,
    Data,
    Bss
}

/// Represents a .text, .rodata, .data, or .bss section
/// that has been loaded and is part of a `LoadedCrate`.
/// The containing `SectionType` enum determines which type of section it is.
pub struct LoadedSection {
    /// The type of this section: .text, .rodata, .data, or .bss.
    pub typ: SectionType,
    /// The full String name of this section, a fully-qualified symbol, 
    /// with the format `<crate>::[<module>::][<struct>::]<fn_name>::<hash>`.
    /// The unique hash is generated for each section by the Rust compiler,
    /// which can be used as a version identifier. 
    /// Not all symbols will have a hash, e.g., ones that are not mangled.
    /// 
    /// # Examples
    /// * `test_lib::MyStruct::new::h843a613894da0c24`
    /// * `my_crate::my_function::hbce878984534ceda`   
    pub name: String,
    /// The `MappedPages` that cover this section.
    pub mapped_pages: Arc<Mutex<MappedPages>>, 
    /// The offset into the `mapped_pages` where this section starts
    pub mapped_pages_offset: usize,
    /// The `VirtualAddress` of this section, cached here as a performance optimization
    /// so we can avoid doing the calculation based on this section's mapped_pages and mapped_pages_offset.
    /// This address value should not be used for accessing this section's data through 
    /// a non-safe dereference or transmute operation. 
    /// Instead, it's just here to help speed up and simply relocations.
    virt_addr: VirtualAddress, 
    /// The size in bytes of this section
    pub size: usize,
    /// Whether or not this section's symbol was exported globally (is public)
    pub global: bool,
    /// The `LoadedCrate` object that contains/owns this section
    pub parent_crate: WeakCrateRef,
    /// The list of sections in foreign crates that this section depends on, i.e., "my required dependencies".
    /// This is kept as a list of strong references because these sections must outlast this section,
    /// i.e., those sections cannot be removed/deleted until this one is deleted.
    pub sections_i_depend_on: Vec<StrongDependency>,
    /// The list of sections in foreign crates that depend on this section, i.e., "my dependents".
    /// This is kept as a list of Weak references because we must be able to remove other sections
    /// that are dependent upon this one before we remove this one.
    /// If we kept strong references to the sections dependent on this one, 
    /// then we wouldn't be able to remove/delete those sections before deleting this one.
    pub sections_dependent_on_me: Vec<WeakDependent>,
    /// We keep track of inter-section dependencies within the same crate
    /// so that we can faithfully reconstruct the crate section's relocation information.
    /// This is necessary for doing a deep copy of the crate in memory, 
    /// without having to re-parse that crate's ELF file (and requiring the ELF file to still exist).
    pub internal_dependencies: Vec<InternalDependency>,
}
impl LoadedSection {
    /// Create a new `LoadedSection`, with an empty `dependencies` list.
    pub fn new(
        typ: SectionType, 
        name: String, 
        mapped_pages: Arc<Mutex<MappedPages>>,
        mapped_pages_offset: usize,
        virt_addr: VirtualAddress,
        size: usize,
        global: bool, 
        parent_crate: WeakCrateRef,
    ) -> LoadedSection {
        LoadedSection::with_dependencies(typ, name, mapped_pages, mapped_pages_offset, virt_addr, size, global, parent_crate, Vec::new(), Vec::new(), Vec::new())
    }

    /// Same as [new()`](#method.new), but uses the given `dependencies` instead of the default empty list.
    pub fn with_dependencies(
        typ: SectionType, 
        name: String, 
        mapped_pages: Arc<Mutex<MappedPages>>,
        mapped_pages_offset: usize,
        virt_addr: VirtualAddress,
        size: usize,
        global: bool, 
        parent_crate: WeakCrateRef,
        sections_i_depend_on: Vec<StrongDependency>,
        sections_dependent_on_me: Vec<WeakDependent>,
        internal_dependencies: Vec<InternalDependency>,
    ) -> LoadedSection {
        LoadedSection {
            typ, name, mapped_pages, mapped_pages_offset, virt_addr, size, global, parent_crate, sections_i_depend_on, sections_dependent_on_me, internal_dependencies
        }
    }

    /// Returns the starting `VirtualAddress` of where this section is loaded into memory. 
    pub fn virt_addr(&self) -> VirtualAddress {
        self.virt_addr
    }

    /// Whether this `LoadedSection` is a .text section
    pub fn is_text(&self) -> bool {
        self.typ == SectionType::Text
    }

    /// Whether this `LoadedSection` is a .rodata section
    pub fn is_rodata(&self) -> bool {
        self.typ == SectionType::Rodata
    }

    /// Whether this `LoadedSection` is a .data section
    pub fn is_data(&self) -> bool {
        self.typ == SectionType::Data
    }

    /// Whether this `LoadedSection` is a .bss section
    pub fn is_bss(&self) -> bool {
        self.typ == SectionType::Bss
    }

    /// Returns the substring of this section's name that excludes the trailing hash. 
    /// 
    /// See the identical associated function [`section_name_without_hash()`](#method.section_name_without_hash) for more. 
    pub fn name_without_hash(&self) -> &str {
        Self::section_name_without_hash(&self.name)
    }


    /// Returns the substring of the given section's name that excludes the trailing hash,
    /// but includes the hash delimiter "`::h`". 
    /// If there is no hash, then it returns the full section name unchanged.
    /// 
    /// # Examples
    /// name: "`keyboard_new::init::h832430094f98e56b`", return value: "`keyboard_new::init::h`"
    /// name: "`start_me`", return value: "`start_me`"
    pub fn section_name_without_hash(sec_name: &str) -> &str {
        // the hash identifier (delimiter) is "::h"
        const HASH_DELIMITER: &'static str = "::h";
        sec_name.rfind("::h")
            .and_then(|end| sec_name.get(0 .. (end + HASH_DELIMITER.len())))
            .unwrap_or_else(|| &sec_name)
    }


    /// Returns the index of the first `WeakDependent` object with a section
    /// that matches the given `matching_section` in this `LoadedSection`'s `sections_dependent_on_me` list.
    pub fn find_weak_dependent(&self, matching_section: &StrongSectionRef) -> Option<usize> {
        for (index, weak_dep) in self.sections_dependent_on_me.iter().enumerate() {
            if let Some(sec_ref) = weak_dep.section.upgrade() {
                if Arc::ptr_eq(matching_section, &sec_ref) {
                    return Some(index);
                }
            }
        }
        None
    }

    /// Copies the actual data contents of this `LoadedSection` to the given `destination_section`. 
    /// The following conditions must be met:    
    /// * The two sections must be from different crates (different parent crates),
    /// * The two sections must have the same size,
    /// * The given `destination_section` must be mapped as writable,
    ///   basically, it must be a .data or .bss section.
    pub (crate) fn copy_section_data_to(&self, destination_section: &mut LoadedSection) -> Result<(), &'static str> {

        let mut dest_sec_mapped_pages = destination_section.mapped_pages.lock();
        let dest_sec_data: &mut [u8] = dest_sec_mapped_pages.as_slice_mut(destination_section.mapped_pages_offset, destination_section.size)?;

        let source_sec_mapped_pages = self.mapped_pages.lock();
        let source_sec_data: &[u8] = source_sec_mapped_pages.as_slice(self.mapped_pages_offset, self.size)?;

        if dest_sec_data.len() == source_sec_data.len() {
            dest_sec_data.copy_from_slice(source_sec_data);
            // debug!("Copied data from source section {:?} {:?} ({:#X}) to dest section {:?} {:?} ({:#X})",
            //     self.typ, self.name, self.size, destination_section.typ, destination_section.name, destination_section.size);
            Ok(())
        }
        else {
            error!("This source section {:?}'s size ({:#X}) is different from the destination section {:?}'s size ({:#X})",
                self.name, self.size, destination_section.name, destination_section.size);
            Err("this source section has a different length than the destination section")
        }
    }
}

impl fmt::Debug for LoadedSection {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "LoadedSection(name: {:?}, vaddr: {:#X})", self.name, self.virt_addr)
    }
}
