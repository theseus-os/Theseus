//! This metadata module contains metadata about all other modules/crates loaded in Theseus.
//! 
//! [This is a good link](https://users.rust-lang.org/t/circular-reference-issue/9097)
//! for understanding why we need `Arc`/`Weak` to handle recursive/circular data structures in Rust. 

use core::ops::{Deref};
use spin::{Mutex, RwLock};
use alloc::{Vec, String, BTreeMap};
use alloc::arc::{Arc, Weak};
use memory::{MappedPages, VirtualAddress, PageTable, MemoryManagementInfo, EntryFlags, FrameAllocator};
use dependency::*;
use super::{TEXT_SECTION_FLAGS, RODATA_SECTION_FLAGS, DATA_BSS_SECTION_FLAGS};

use super::SymbolMap;

/// A Strong reference (`Arc`) to a `LoadedSection`.
pub type StrongSectionRef  = Arc<Mutex<LoadedSection>>;
/// A Weak reference (`Weak`) to a `LoadedSection`.
pub type WeakSectionRef = Weak<Mutex<LoadedSection>>;

/// A Strong reference (`Arc`) to a `LoadedCrate`.
pub type StrongCrateRef  = Arc<LoadedCrate>; // Arc<RwLock<LoadedCrate>>;
/// A Weak reference (`Weak`) to a `LoadedCrate`.
pub type WeakCrateRef = Weak<LoadedCrate>; // Weak<RwLock<LoadedCrate>>;


#[derive(PartialEq)]
pub enum CrateType {
    Kernel,
    Application,
    Userspace,
}
impl CrateType {
    pub fn prefix(&self) -> &'static str {
        match self {
            CrateType::Kernel       => "__k_",
            CrateType::Application  => "__a_",
            CrateType::Userspace    => "__u_",
        }
    }

    /// Returns a tuple of (CrateType, &str) based on the given `module_name`,
    /// in which the `&str` is the rest of the module name after the prefix. 
    /// # Examples 
    /// ```
    /// let result = CrateType::from_module_name("__k_my_crate");
    /// assert_eq!(result, (CrateType::Kernel, "my_crate") );
    /// ```
    pub fn from_module_name<'a>(module_name: &'a str) -> Result<(CrateType, &'a str), &'static str> {
        if module_name.starts_with(CrateType::Application.prefix()) {
            Ok((
                CrateType::Application,
                module_name.get(CrateType::Application.prefix().len() .. ).ok_or("Couldn't get name of application module")?
            ))
        }
        else if module_name.starts_with(CrateType::Kernel.prefix()) {
            Ok((
                CrateType::Kernel,
                module_name.get(CrateType::Kernel.prefix().len() .. ).ok_or("Couldn't get name of kernel module")?
            ))
        }
        else if module_name.starts_with(CrateType::Userspace.prefix()) {
            Ok((
                CrateType::Userspace,
                module_name.get(CrateType::Userspace.prefix().len() .. ).ok_or("Couldn't get name of userspace module")?
            ))
        }
        else {
            Err("module_name didn't start with a known CrateType prefix")
        }
    }


    pub fn is_application(module_name: &str) -> bool {
        module_name.starts_with(CrateType::Application.prefix())
    }

    pub fn is_kernel(module_name: &str) -> bool {
        module_name.starts_with(CrateType::Kernel.prefix())
    }

    pub fn is_userspace(module_name: &str) -> bool {
        module_name.starts_with(CrateType::Userspace.prefix())
    }
}




/// Represents a single crate object file that has been loaded into the system.
pub struct LoadedCrate {
    /// The name of this crate. Wrapped in a RwLock to support overriding the name later.
    pub crate_name: RwLock<String>,
    /// A map containing all the sections in this crate.
    /// In general we're only interested the values (the `LoadedSection`s themselves),
    /// but we keep each section's shndx (section header index from its crate's ELF file)
    /// as the key because it helps us quickly handle relocations and crate swapping.
    pub sections: BTreeMap<usize, StrongSectionRef>,
    /// The `MappedPages` that include the text sections for this crate,
    /// i.e., sections that are readable and executable, but not writable.
    pub text_pages: Option<Arc<RwLock<MappedPages>>>,
    /// The `MappedPages` that include the rodata sections for this crate.
    /// i.e., sections that are read-only, not writable nor executable.
    pub rodata_pages: Option<Arc<RwLock<MappedPages>>>,
    /// The `MappedPages` that include the data and bss sections for this crate.
    /// i.e., sections that are readable and writable but not executable.
    pub data_pages: Option<Arc<RwLock<MappedPages>>>,

    // crate_dependencies: Vec<LoadedCrate>,
}

use core::fmt;
impl fmt::Debug for LoadedCrate {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "LoadedCrate{{{}}}", self.crate_name.read().deref())
    }
}

impl Drop for LoadedCrate {
    fn drop(&mut self) {
        trace!("### Dropped LoadedCrate: {}", self.crate_name.read().deref());
    }
}

impl LoadedCrate {
    /// Returns the `LoadedSection` of type `SectionType::Text` that matches the requested function name, if it exists in this `LoadedCrate`.
    /// Only matches demangled names, e.g., "my_crate::foo".
    pub fn get_function_section(&self, func_name: &str) -> Option<StrongSectionRef> {
        self.find_section(|sec| sec.is_text() && sec.name == func_name)
    }

    /// Returns the first `LoadedSection` that matches the given predicate
    pub fn find_section<F>(&self, predicate: F) -> Option<StrongSectionRef> 
        where F: Fn(&LoadedSection) -> bool
    {
        self.sections.values().filter(|sec_ref| {
            let sec = sec_ref.lock();
            predicate(&sec)
        }).next().cloned()
    }


    pub fn crates_i_depend_on(&self) -> Vec<StrongCrateRef> {
        unimplemented!();
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
    /// and that would result in weird inconsistencies that violate thos dependencies.
    pub fn deep_copy<A: FrameAllocator>(
        &self, 
        kernel_mmi: &mut MemoryManagementInfo, 
        allocator: &mut A
    ) -> Result<StrongCrateRef, &'static str> {
        // deep copy all of the memory regions.
        // we initially map the as writable because we'll have to copy things into them
        let (mut new_text_pages, mut new_rodata_pages, mut new_data_pages) = {
            if let PageTable::Active(ref mut active_table) = kernel_mmi.page_table {
                let new_text_pages = match self.text_pages {
                    Some(ref tp) => Some(tp.read().deep_copy(Some(TEXT_SECTION_FLAGS() | EntryFlags::WRITABLE), active_table, allocator)?),
                    None => None,
                };
                let new_rodata_pages = match self.rodata_pages {
                    Some(ref rp) => Some(rp.read().deep_copy(Some(RODATA_SECTION_FLAGS() | EntryFlags::WRITABLE), active_table, allocator)?),
                    None => None,
                };
                let new_data_pages = match self.data_pages {
                    Some(ref dp) => Some(dp.read().deep_copy(Some(DATA_BSS_SECTION_FLAGS()), active_table, allocator)?),
                    None => None,
                };
                (new_text_pages, new_rodata_pages, new_data_pages)
            }
            else {
                return Err("couldn't get kernel's active page table");
            }
        };

        // deep copy the list of sections
        let new_sections = self.sections.clone();

        // Now that we cloned the actual map of sections, we need to go back through it
        // and fix up the things in each `LoadedSection` that don't make sense to just "clone": 
        // 1) The parent_crate reference itself, since we're replacing that with a new one (this is done at the end of this function),
        // 2) The section's virt_addr, which is a performance optimization that simply caches 
        //    the virtual address value calculated from its mapped_pages and mapped_pages_offset,
        // 3) The foreign sections dependencies (sections_i_depend_on and sections_dependent_on_me),
        // 4) Every relocation entry needs to be rewritten because all of the virtual addresses have changed.
        for new_sec_ref in new_sections.values() {
            let mut new_sec = new_sec_ref.lock();
            let new_sec_mapped_pages = match new_sec.typ {
                SectionType::Text   => new_text_pages.as_mut().ok_or_else(|| "missing text pages in newly-copied crate")?,
                SectionType::Rodata => new_rodata_pages.as_mut().ok_or_else(|| "missing rodata pages in newly-copied crate")?,
                SectionType::Data |
                SectionType::Bss    => new_data_pages.as_mut().ok_or_else(|| "missing data pages in newly-copied crate")?,
            };
            let new_sec_mapped_pages_offset = new_sec.mapped_pages_offset;

            // no sections can possibly depend on this one, since we just created it
            new_sec.sections_dependent_on_me.clear();

            // This crate still depends on the same sections (at least until we change that later),
            // so we keep those as is, but we do need to recalculate those relocations.
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
                let source_sec_vaddr = new_sections.get(&internal_dep.source_sec_shndx)
                    .ok_or_else(|| "Couldn't get new section specified by an internal dependency's source_sec_shndx")?
                    .lock()
                    .virt_addr();
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
            if let Some(ref mut tp) = new_text_pages { 
                try!(tp.remap(active_table, TEXT_SECTION_FLAGS()));
            }
            if let Some(ref mut rp) = new_rodata_pages { 
                try!(rp.remap(active_table, RODATA_SECTION_FLAGS()));
            }
            // data/bss sections are already mapped properly, since they're supposed to be writable
        }
        else {
            return Err("couldn't get kernel's active page table");
        }

        let new_text_pages = new_text_pages.map(|mp| Arc::new(RwLock::new(mp)));
        let new_rodata_pages = new_rodata_pages.map(|mp| Arc::new(RwLock::new(mp)));
        let new_data_pages = new_data_pages.map(|mp| Arc::new(RwLock::new(mp)));

        let new_crate = Arc::new(LoadedCrate {
            crate_name: RwLock::new(self.crate_name.read().clone()),
            sections: new_sections,
            text_pages: new_text_pages.clone(),
            rodata_pages: new_rodata_pages.clone(),
            data_pages: new_data_pages.clone(),
        });

        // Update the sections to point to their new parent crate
        // and to point to their new MappedPages 
        let new_crate_weak_ref = Arc::downgrade(&new_crate);
        for sec in new_crate.sections.values() {
            let mut sec_locked = sec.lock();
            sec_locked.parent_crate = new_crate_weak_ref.clone();
            sec_locked.mapped_pages = match sec_locked.typ {
                SectionType::Text   => new_text_pages.clone().ok_or_else(|| "missing text pages in newly-copied crate")?,
                SectionType::Rodata => new_rodata_pages.clone().ok_or_else(|| "missing text pages in newly-copied crate")?,
                SectionType::Data |
                SectionType::Bss    => new_data_pages.clone().ok_or_else(|| "missing data pages in newly-copied crate")?,
            }
        }

        Ok(new_crate)
    }
}


/// Returns a map containing all of the global symbols 
/// in the given section iterator (if Some), otherwise in this crate's sections list.
pub fn global_symbol_map<'a, I>(sections: I, crate_name: &str) -> SymbolMap 
    where I: IntoIterator<Item = &'a StrongSectionRef> 
{
    symbol_map(sections, crate_name, |sec| sec.global)
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
    let mut map: SymbolMap = BTreeMap::new();
    for sec in sections.into_iter().filter(|sec| predicate(sec.lock().deref())) {
        let key = sec.lock().name.clone();
        if let Some(old_val) = map.insert(key.clone(), Arc::downgrade(&sec)) {
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
#[derive(Debug)]
pub struct LoadedSection {
    /// The type of this section: .text, .rodata, .data, or .bss.
    pub typ: SectionType,
    /// The full String name of this section, a fully-qualified symbol, 
    /// e.g., `<crate>::<module>::<struct>::<fn_name>`
    /// For example, test_lib::MyStruct::new
    pub name: String,
    /// the unique hash generated for this section by the Rust compiler,
    /// which can be used as a version identifier. 
    /// Not all symbols will have a hash, like those that are not mangled.
    pub hash: Option<String>,
    /// The `MappedPages` that cover this section.
    pub mapped_pages: Arc<RwLock<MappedPages>>, 
    /// The offset into the `parent_crate`'s `MappedPages` where this section starts
    pub mapped_pages_offset: usize,
    /// The `VirtualAddress` of this section, cached here as a performance optimization
    /// so we can avoid doing the calculation based on this section's mapped_pages and mapped_pages_offset.
    /// This address value should not be used for accessing this section's data through an unsafe dereference,
    /// rather it's just here to help speed up and simply relocations.
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
        hash: Option<String>, 
        mapped_pages: Arc<RwLock<MappedPages>>,
        mapped_pages_offset: usize,
        virt_addr: VirtualAddress,
        size: usize,
        global: bool, 
        parent_crate: WeakCrateRef,
    ) -> LoadedSection {
        LoadedSection::with_dependencies(typ, name, hash, mapped_pages, mapped_pages_offset, virt_addr, size, global, parent_crate, Vec::new(), Vec::new(), Vec::new())
    }

    /// Same as [new()`](#method.new), but uses the given `dependencies` instead of the default empty list.
    pub fn with_dependencies(
        typ: SectionType, 
        name: String, 
        hash: Option<String>, 
        mapped_pages: Arc<RwLock<MappedPages>>,
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
            typ, name, hash, mapped_pages, mapped_pages_offset, virt_addr, size, global, parent_crate, sections_i_depend_on, sections_dependent_on_me, internal_dependencies
        }
    }

    // /// Returns a reference to the `MappedPages` that covers this `LoadedSection`.
    // /// Because that `MappedPages` object is owned by this `LoadedSection`'s `parent_crate`,
    // /// the lifetime of the returned `MappedPages` reference is tied to the lifetime 
    // /// of the given `LoadedCrate` parent crate object.
    // /// The given `parent_crate` reference should be obtained by invoking 
    // /// the [`parent_crate()`](#method.parent_crate) method. 
    // pub fn mapped_pages<'a>(&self, parent_crate: &'a LoadedCrate) -> Option<&'a MappedPages> {
    //     match self.typ {
    //         SectionType::Text   => parent_crate.text_pages.as_ref(),
    //         SectionType::Rodata => parent_crate.rodata_pages.as_ref(),
    //         SectionType::Data |
    //         SectionType::Bss    => parent_crate.data_pages.as_ref(),
    //     }
    // }

    // /// Returns a mutable reference to the `MappedPages` that covers this `LoadedSection`.
    // /// Because that `MappedPages` object is owned by this `LoadedSection`'s `parent_crate`,
    // /// the lifetime of the returned `MappedPages` reference is tied to the lifetime 
    // /// of the given `LoadedCrate` parent crate object. 
    // /// The given `parent_crate` reference should be obtained by invoking 
    // /// the [`parent_crate_mut()`](#method.parent_crate_mut) method.
    // pub fn mapped_pages_mut<'a>(&self, parent_crate: &'a mut LoadedCrate) -> Option<&'a mut MappedPages> {
    //     match self.typ {
    //         SectionType::Text   => parent_crate.text_pages.as_mut(),
    //         SectionType::Rodata => parent_crate.rodata_pages.as_mut(),
    //         SectionType::Data |
    //         SectionType::Bss    => parent_crate.data_pages.as_mut(),
    //     }
    // }

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

    /// Returns the index of the first `StrongDependency` object with a section
    /// that matches the given `matching_section` in this `LoadedSection`'s `sections_i_depend_on` list.
    pub fn find_strong_dependency(&self, matching_section: &StrongSectionRef) -> Option<usize> {
        for (index, strong_dep) in self.sections_i_depend_on.iter().enumerate() {
            if Arc::ptr_eq(matching_section, &strong_dep.section) {
                return Some(index);
            }
        }
        None
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

        let mut dest_sec_mapped_pages = destination_section.mapped_pages.write();
        let dest_sec_data: &mut [u8] = dest_sec_mapped_pages.as_slice_mut(destination_section.mapped_pages_offset, destination_section.size)?;

        let source_sec_mapped_pages = self.mapped_pages.read();
        let source_sec_data: &[u8] = source_sec_mapped_pages.as_slice(self.mapped_pages_offset, self.size)?;

        if dest_sec_data.len() == source_sec_data.len() {
            dest_sec_data.copy_from_slice(source_sec_data);
            debug!("Copied data from source section {:?} {:?} ({:#X}) to dest section {:?} {:?} ({:#X})",
                self.typ, self.name, self.size, destination_section.typ, destination_section.name, destination_section.size);
            Ok(())
        }
        else {
            error!("This source section {:?}'s size ({:#X}) is different from the destination section {:?}'s size ({:#X})",
                self.name, self.size, destination_section.name, destination_section.size);
            Err("this source section has a different length than the destination section")
        }
    }
}

impl Drop for LoadedSection {
    fn drop(&mut self) {
        trace!("### Dropped LoadedSection {:?} {:?}", self.typ, self.name);
    }
}
