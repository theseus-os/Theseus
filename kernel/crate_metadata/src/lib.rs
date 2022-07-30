//! Defines types that contain metadata about crates loaded in Theseus and their dependencies.
//! 
//! ## Representing dependencies between sections
//! If one section `A` references or uses another section `B`, 
//! then we colloquially say that *`A` depends on `B`*. 
//! 
//! In this scenario, `A` has a [`StrongDependency`] on `B`,
//! and `B` has a [`WeakDependent`] pointing back to `A`. 
//! 
//! Assuming `A` and `B` are both [`LoadedSection`] objects,
//! then [`A.sections_i_depend_on`] includes a `StrongDependency(B)`
//! and [`B.sections_dependent_on_me`] includes a `WeakDependent(A)`.
//!  
//! In this way, the dependency graphs are fully associative,
//! allowing a given [`LoadedSection`] to easily find 
//! both its dependencies and its dependents instantly.
//! 
//! More importantly, it allows `A` to be dropped before `B`, 
//! but not the other way around. 
//! This correctly avoids dependency violations by ensuring that a section `B`
//! is never dropped while any other section `A` relies on it.
//! 
//! When swapping crates, the [`WeakDependent`]s are actually more useful. 
//! For example, if we want to swap the crate that contains section `B1` with a new one `B2`, 
//! then we can immediately find all of the section `A`s that depend on `B1` 
//! by iterating over `B1.sections_dependent_on_me`. 
//! To complete the swap and fully replace `B1` with `B2`, 
//! we would do the following (pseudocode):
//! ```
//! for secA in B1.sections_dependent_on_me {     
//!     change secA's relocation to point to B1     
//!     add WeakDependent(secA) to B2.sections_dependent_on_me     
//!     remove StrongDependency(B1) from secA.sections_i_depend_on     
//!     add StrongDependency(B2) to secA.sections_i_depend_on      
//!     remove WeakDependent(secA) from B1.sections_dependent_on_me (current iterator)     
//! }
//! ```
//! 
//! [`A.sections_i_depend_on`]: LoadedSectionInner::sections_i_depend_on
//! [`B.sections_dependent_on_me`]: LoadedSectionInner::sections_dependent_on_me
//! 

#![deny(unsafe_op_in_unsafe_fn)]
#![no_std]

extern crate alloc;

use core::convert::TryFrom;
use core::fmt;
use core::ops::Range;
use log::{error, debug, trace};
use spin::{Mutex, RwLock, Once};
use alloc::{
    collections::BTreeSet,
    format,
    string::String,
    sync::{Arc, Weak},
    vec::Vec,
};
use memory::{MappedPages, VirtualAddress, EntryFlags};
#[cfg(internal_deps)]
use memory::PageTable;
use cow_arc::{CowArc, CowWeak};
use fs_node::{FileRef, WeakFileRef};
use hashbrown::HashMap;
use goblin::elf::reloc::*;
pub use str_ref::StrRef;


/// A Strong reference to a [`LoadedCrate`].
pub type StrongCrateRef  = CowArc<LoadedCrate>;
/// A Weak reference to a [`LoadedCrate`].
pub type WeakCrateRef = CowWeak<LoadedCrate>;
/// A Strong reference ([`Arc`]) to a [`LoadedSection`].
pub type StrongSectionRef  = Arc<LoadedSection>;
/// A Weak reference ([`Weak`]) to a [`LoadedSection`].
pub type WeakSectionRef = Weak<LoadedSection>;
/// A Section Header iNDeX (SHNDX), as specified by the ELF format. 
/// Even though this is typically encoded as a `u16`, its decoded form can exceed the max size of `u16`.
pub type Shndx = usize;


/// `.text` sections are read-only and executable.
pub const TEXT_SECTION_FLAGS:     EntryFlags = EntryFlags::PRESENT;
/// `.rodata` sections are read-only and non-executable.
pub const RODATA_SECTION_FLAGS:   EntryFlags = EntryFlags::from_bits_truncate(EntryFlags::PRESENT.bits() | EntryFlags::NO_EXECUTE.bits());
/// `.data` and `.bss` sections are read-write and non-executable.
pub const DATA_BSS_SECTION_FLAGS: EntryFlags = EntryFlags::from_bits_truncate(EntryFlags::PRESENT.bits() | EntryFlags::NO_EXECUTE.bits() | EntryFlags::WRITABLE.bits());


/// The Theseus Makefile appends prefixes onto bootloader module names,
/// which are separated by the "#" character. 
/// For example, "k#my_crate-hash.o".
pub const MODULE_PREFIX_DELIMITER: &'static str = "#";
/// A crate's name and its hash are separated by "-", i.e., "my_crate-hash".
pub const CRATE_HASH_DELIMITER: &'static str = "-";
/// A section's demangled name and its hash are separated by "::h", 
/// e.g., "my_crate::section_name::h<hash>".
pub const SECTION_HASH_DELIMITER: &'static str = "::h";


/// The type of a crate, based on its object file naming convention.
/// This naming convention is only used for crate object files
/// that come from **bootloader-provided modules**,
/// which the Theseus makefile assigns at build time.
/// 
/// See the `from_module_name()` function for more. 
#[derive(Debug, PartialEq)]
pub enum CrateType {
    Kernel,
    Application,
    Userspace,
    Executable,
}
impl CrateType {
    fn first_char(&self) -> &'static str {
        match self {
            CrateType::Kernel       => "k",
            CrateType::Application  => "a",
            CrateType::Userspace    => "u",
            CrateType::Executable   => "e",
        }
    }
    
    /// Returns the string suffix for use as the name 
    /// of the crate object file's containing namespace.
    pub fn default_namespace_name(&self) -> &'static str {
        match self {
            CrateType::Kernel       => "_kernel",
            CrateType::Application  => "_applications",
            CrateType::Userspace    => "_userspace",
            CrateType::Executable   => "_executables",
        }
    }
    
    /// Returns a tuple of (CrateType, &str, &str) based on the given `module_name`, in which:
    /// 1. the `CrateType` is based on the first character,
    /// 2. the first `&str` is the namespace prefix, e.g., `"sse"` in `"k_sse#..."`,
    /// 3. the second `&str` is the rest of the module file name after the prefix delimiter `"#"`.
    /// 
    /// # Examples 
    /// ```
    /// let result = CrateType::from_module_name("k#my_crate.o");
    /// assert_eq!(result, (CrateType::Kernel, "", "my_crate.o") );
    /// 
    /// let result = CrateType::from_module_name("ksse#my_crate.o");
    /// assert_eq!(result, (CrateType::Kernel, "sse", "my_crate.o") );
    /// ```
    pub fn from_module_name<'a>(module_name: &'a str) -> Result<(CrateType, &'a str, &'a str), &'static str> {
        let mut iter = module_name.split(MODULE_PREFIX_DELIMITER);
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
        else if prefix.starts_with(CrateType::Executable.first_char()) {
            Ok((CrateType::Executable, namespace_prefix, crate_name))
        }
        else {
            error!("module_name {:?} didn't start with a known CrateType prefix", module_name);
            Err("module_name didn't start with a known CrateType prefix")
        }
    }
}


/// Represents a single crate whose object file has been 
/// loaded and linked into at least one `CrateNamespace`.
pub struct LoadedCrate {
    /// The name of this crate.
    pub crate_name: StrRef,
    /// The object file that this crate was loaded from.
    pub object_file: FileRef,
    /// The file that contains debug symbols for this crate. 
    /// Debug symbols may exist in several forms:
    /// * In the same file as the `object_file` above, i.e., not stripped,
    /// * As a separate file that was stripped off from the original object file,
    /// * Not at all (no debug symbols available for this crate).
    /// 
    /// By default, the constructor for `LoadedCrate` assumes the first form,
    /// so it will initialize this to a weak reference to the `LoadedCrate`'s `object_file` field.
    /// If that is not the case, then this field should be set differently once the crate is initialized
    /// or once a debug symbol file becomes available or requested.
    pub debug_symbols_file: WeakFileRef,
    /// A map containing all the sections in this crate.
    /// In general we're only interested the values (the `LoadedSection`s themselves),
    /// but we keep each section's shndx (section header index from its crate's ELF file)
    /// as the key because it helps us quickly handle relocations and crate swapping.
    pub sections: HashMap<Shndx, StrongSectionRef>,
    /// A tuple of:    
    /// 1. The `MappedPages` that contain sections that are readable and executable, but not writable,
    ///     i.e., the `.text` sections for this crate,
    /// 2. The range of virtual addresses covered by this mapping.
    pub text_pages: Option<(Arc<Mutex<MappedPages>>, Range<VirtualAddress>)>,
    /// A tuple of:    
    /// 1. The `MappedPages` that contain sections that are read-only, not writable nor executable,
    ///     i.e., the `.rodata`, `.eh_frame`, and `.gcc_except_table` sections for this crate,
    /// 2. The range of virtual addresses covered by this mapping.
    pub rodata_pages: Option<(Arc<Mutex<MappedPages>>, Range<VirtualAddress>)>,
    /// A tuple of:    
    /// 1. The `MappedPages` that contain sections that are readable and writable but not executable,
    ///     i.e., the `.data` and `.bss` sections for this crate,
    /// 2. The range of virtual addresses covered by this mapping.
    pub data_pages: Option<(Arc<Mutex<MappedPages>>, Range<VirtualAddress>)>,
    
    // The fields below are most used to accelerate crate swapping,
    // and are not strictly necessary just for normal crate usage and management.

    /// The set of global symbols in this crate, including regular ones 
    /// that are prefixed with the `crate_name` and `no_mangle` symbols that are not.
    /// The `Shndx` values in this set are the section index (shndx) numbers, 
    /// which can be used as the key to look up the actual `LoadedSection` in the `sections` list above.
    pub global_sections: BTreeSet<Shndx>,
    /// The set of thread-local storage (TLS) symbols in this crate.
    /// The `Shndx` values in this set are the section index (shndx) numbers, 
    /// which can be used as the key to look up the actual `LoadedSection` in the `sections` list above.
    pub tls_sections: BTreeSet<Shndx>,
    /// The set of `.data` and `.bss` sections in this crate.
    /// The `Shndx` values in this set are the section index (shndx) numbers, 
    /// which can be used as the key to look up the actual `LoadedSection` in the `sections` list above.
    pub data_sections: BTreeSet<Shndx>,
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
    /// because this crate will only have populated its `global_sections` set during loading. 
    pub reexported_symbols: BTreeSet<StrRef>,
}

impl fmt::Debug for LoadedCrate {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("LoadedCrate")
            .field("name", &self.crate_name)
            .field("object_file", &self.object_file.try_lock()
                .map(|f| f.get_absolute_path())
                .unwrap_or_else(|| format!("<Locked>"))
            )
            .finish_non_exhaustive()
    }
}

impl Drop for LoadedCrate {
    fn drop(&mut self) {
        #[cfg(not(downtime_eval))]
        trace!("### Dropped LoadedCrate: {}", self.crate_name);
    }
}

impl LoadedCrate {
    /// Returns the `LoadedSection` of type `SectionType::Text` that matches the requested function name, if it exists in this `LoadedCrate`.
    /// Only matches demangled names, e.g., "my_crate::foo".
    pub fn get_function_section(&self, func_name: &str) -> Option<&StrongSectionRef> {
        self.find_section(|sec| 
            sec.get_type() == SectionType::Text &&
            sec.name.as_str() == func_name
        )
    }

    /// A convenience function to iterate over only the data (.data or .bss) sections in this crate.
    pub fn data_sections_iter(&self) -> impl Iterator<Item = &StrongSectionRef> {
        self.data_sections
            .iter()
            .filter_map(move |shndx| self.sections.get(shndx))
    }

    /// A convenience function to iterate over only the global (public) sections in this crate.
    pub fn global_sections_iter(&self) -> impl Iterator<Item = &StrongSectionRef> {
        self.global_sections
            .iter()
            .filter_map(move |shndx| self.sections.get(shndx))
    }

    /// Returns the **first** `LoadedSection` that matches the given predicate,
    /// i.e., for which the `predicate` closure returns `true`.
    /// 
    /// If you need to check for multiple matches, then it's best to iterate
    /// over the sections in this crate yourself. 
    pub fn find_section<F>(&self, predicate: F) -> Option<&StrongSectionRef> 
        where F: Fn(&LoadedSection) -> bool
    {
        self.sections.values()
            .find(|&sec| predicate(sec))
    }

    /// Returns the substring of this crate's name that excludes the trailing hash. 
    /// If there is no hash, then it returns the entire name. 
    pub fn crate_name_without_hash(&self) -> &str {
        self.crate_name.split(CRATE_HASH_DELIMITER)
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
            for weak_dep in &sec.inner.read().sections_dependent_on_me {
                if let Some(dep_sec) = weak_dep.section.upgrade() {
                    results.push(dep_sec.parent_crate.clone());
                }
            }
        }
        results
    }


    /// Returns the set of crates that this crate depends on. 
    /// Only includes direct dependencies "one hop" away, 
    /// not recursive dependencies "multiples hops" away.
    /// 
    /// Currently, the list may include duplicates.
    /// The caller is responsible for filtering out duplicates when using the list.
    pub fn crates_i_depend_on(&self) -> Vec<WeakCrateRef> {
        let mut results: Vec<WeakCrateRef> = Vec::new();
        for sec in self.sections.values() {
            for strong_dep in &sec.inner.read().sections_i_depend_on {
                results.push(strong_dep.section.parent_crate.clone());
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
    /// 
    /// This is only available when the `internal_deps` cfg option is set.
    #[cfg(internal_deps)]
    pub fn deep_copy(
        &self, 
        page_table: &mut PageTable, 
    ) -> Result<StrongCrateRef, &'static str> {

        // This closure deep copies the given mapped_pages (mapping them as WRITABLE)
        // and recalculates the the range of addresses covered by the new mapping.
        let mut deep_copy_mp = |old_mp_range: &(Arc<Mutex<MappedPages>>, Range<VirtualAddress>), flags: EntryFlags|
            -> Result<(Arc<Mutex<MappedPages>>, Range<VirtualAddress>), &'static str> 
        {
            let old_mp_locked = old_mp_range.0.lock();
            let old_start_address = old_mp_range.1.start.value();
            let size = old_mp_range.1.end.value() - old_start_address;
            let offset = old_start_address - old_mp_locked.start_address().value();
            let new_mp = old_mp_range.0.lock().deep_copy(Some(flags | EntryFlags::WRITABLE), page_table)?;
            let new_start_address = new_mp.start_address() + offset;
            Ok((Arc::new(Mutex::new(new_mp)), new_start_address .. (new_start_address + size)))
        };

        // First, deep copy all of the memory regions.
        // We initially map the as writable because we'll have to copy things into them
        let (new_text_pages_range, new_rodata_pages_range, new_data_pages_range) = {
            let new_text_pages = match self.text_pages {
                Some(ref tp) => Some(deep_copy_mp(tp, TEXT_SECTION_FLAGS)?),
                None => None,
            };
            let new_rodata_pages = match self.rodata_pages {
                Some(ref rp) => Some(deep_copy_mp(rp, RODATA_SECTION_FLAGS)?),
                None => None,
            };
            let new_data_pages = match self.data_pages {
                Some(ref dp) => Some(deep_copy_mp(dp, DATA_BSS_SECTION_FLAGS)?),
                None => None,
            };
            (new_text_pages, new_rodata_pages, new_data_pages)
        };

        let new_text_pages_ref   = new_text_pages_range.clone().map(|tup| tup.0);
        let new_rodata_pages_ref = new_rodata_pages_range.clone().map(|tup| tup.0);
        let new_data_pages_ref   = new_data_pages_range.clone().map(|tup| tup.0);

        let new_crate = CowArc::new(LoadedCrate {
            crate_name:              self.crate_name.clone(),
            object_file:             self.object_file.clone(),
            debug_symbols_file:      self.debug_symbols_file.clone(),
            sections:                HashMap::new(),
            text_pages:              new_text_pages_range,
            rodata_pages:            new_rodata_pages_range,
            data_pages:              new_data_pages_range,
            global_sections:         self.global_sections.clone(),
            tls_sections:            self.tls_sections.clone(),
            data_sections:           self.data_sections.clone(),
            reexported_symbols:      self.reexported_symbols.clone(),
        });
        let new_crate_weak_ref = CowArc::downgrade(&new_crate);

        let mut new_text_pages_locked   = new_text_pages_ref  .as_ref().map(|tp| tp.lock());
        let mut new_rodata_pages_locked = new_rodata_pages_ref.as_ref().map(|rp| rp.lock());
        let mut new_data_pages_locked   = new_data_pages_ref  .as_ref().map(|dp| dp.lock());

        // Second, deep copy the entire list of sections and fix things that don't make sense to directly clone:
        // 1) The parent_crate reference itself, since we're replacing that with a new one,
        // 2) The section's mapped_pages, which will point to a new `MappedPages` object for the newly-copied crate,
        // 3) The section's virt_addr, which is based on its new mapped_pages
        let mut new_sections: HashMap<Shndx, StrongSectionRef> = HashMap::new();
        for (shndx, old_sec) in self.sections.iter() {
            let old_sec_inner = old_sec.inner.read();
            let new_sec_mapped_pages_offset = old_sec.mapped_pages_offset;
            let (new_sec_mapped_pages_ref, new_sec_virt_addr) = match old_sec.typ {
                SectionType::Text => (
                    new_text_pages_ref.clone().ok_or_else(|| "BUG: missing text pages in newly-copied crate")?,
                    new_text_pages_locked.as_ref().and_then(|tp| tp.address_at_offset(new_sec_mapped_pages_offset)),
                ),
                SectionType::Rodata
                | SectionType::GccExceptTable
                | SectionType::TlsBss
                | SectionType::TlsData 
                | SectionType::EhFrame => (
                    new_rodata_pages_ref.clone().ok_or_else(|| "BUG: missing rodata pages in newly-copied crate")?,
                    new_rodata_pages_locked.as_ref().and_then(|rp| rp.address_at_offset(new_sec_mapped_pages_offset)),
                ),
                SectionType::Data
                | SectionType::Bss => (
                    new_data_pages_ref.clone().ok_or_else(|| "BUG: missing data pages in newly-copied crate")?,
                    new_data_pages_locked.as_ref().and_then(|dp| dp.address_at_offset(new_sec_mapped_pages_offset)),
                ),
            };
            let new_sec_virt_addr = new_sec_virt_addr.ok_or_else(|| "BUG: couldn't get virt_addr for new section")?;

            let new_sec = Arc::new(LoadedSection::with_dependencies(
                old_sec.typ,                            // section type is the same
                old_sec.name.clone(),                   // name is the same
                new_sec_mapped_pages_ref,               // mapped_pages is different, points to the new duplicated one
                new_sec_mapped_pages_offset,            // mapped_pages_offset is the same
                new_sec_virt_addr,                      // virt_addr is different, based on the new mapped_pages
                old_sec.size(),                         // size is the same
                old_sec.global,                         // globalness is the same
                new_crate_weak_ref.clone(),             // parent_crate is different, points to the newly-copied crate
                old_sec_inner.sections_i_depend_on.clone(),   // dependencies are the same, but relocations need to be re-written
                Vec::new(),                             // no sections can possibly depend on this one, since we just created it
                old_sec_inner.internal_dependencies.clone()   // internal dependencies are the same, but relocations need to be re-written
            ));

            new_sections.insert(*shndx, new_sec);
        }


        // Now we can go through the list again and fix up the rest of the elements in each section.
        // The foreign sections dependencies (sections_i_depend_on) are the same, 
        // but all relocation entries must be rewritten because the sections' virtual addresses have changed.
        for new_sec in new_sections.values() {
            let mut new_sec_inner = new_sec.inner.write();
            let new_sec_mapped_pages = match new_sec.typ {
                SectionType::Text => new_text_pages_locked
                    .as_mut()
                    .ok_or_else(|| "BUG: missing text pages in newly-copied crate")?,
                SectionType::Rodata
                | SectionType::TlsBss
                | SectionType::TlsData
                | SectionType::GccExceptTable
                | SectionType::EhFrame => new_rodata_pages_locked
                    .as_mut()
                    .ok_or_else(|| "BUG: missing rodata pages in newly-copied crate")?,
                SectionType::Data
                | SectionType::Bss => new_data_pages_locked
                    .as_mut()
                    .ok_or_else(|| "BUG: missing data pages in newly-copied crate")?,
            };
            let new_sec_mapped_pages_offset = new_sec.mapped_pages_offset;

            // The newly-duplicated crate still depends on the same sections, so we keep those as is, 
            // but we do need to recalculate those relocations.
            for strong_dep in new_sec_inner.sections_i_depend_on.iter_mut() {
                // we can skip modifying "absolute" relocations, since those only depend on the source section,
                // which we haven't actually changed (we've duplicated the target section here, not the source)
                if !strong_dep.relocation.is_absolute() {
                    let source_sec = &strong_dep.section;
                    // perform the actual fix by writing the relocation
                    write_relocation(
                        strong_dep.relocation, 
                        new_sec_mapped_pages, 
                        new_sec_mapped_pages_offset,
                        source_sec.start_address(),
                        true
                    )?;

                    // add this new_sec as one of the source sec's weak dependents
                    source_sec.inner.write().sections_dependent_on_me.push(
                        WeakDependent {
                            section: Arc::downgrade(new_sec),
                            relocation: strong_dep.relocation,
                        }
                    );
                }
            }

            // Finally, fix up all of its internal dependencies by recalculating/rewriting their relocations.
            // We shouldn't need to actually change the InternalDependency instances themselves 
            // because they are based on crate-specific section shndx values, 
            // which are completely safe to clone without needing any fix ups. 
            for internal_dep in &new_sec.inner.read().internal_dependencies {
                let source_sec = new_sections.get(&internal_dep.source_sec_shndx)
                    .ok_or_else(|| "Couldn't get new section specified by an internal dependency's source_sec_shndx")?;

                // The source and target (new_sec) sections might be the same, so we need to check first
                // to ensure that we don't cause deadlock by trying to lock the same section twice.
                let source_sec_vaddr = if Arc::ptr_eq(source_sec, new_sec) {
                    // here: the source_sec and new_sec are the same, so just use the already-locked new_sec
                    new_sec.start_address()
                } else {
                    // here: the source_sec and new_sec are different, so we can go ahead and safely lock the source_sec
                    source_sec.start_address()
                };
                write_relocation(
                    internal_dep.relocation, 
                    new_sec_mapped_pages, 
                    new_sec_mapped_pages_offset,
                    source_sec_vaddr,
                    true
                )?;
            }
        }

        // since we mapped all the new MappedPages as writable, we need to properly remap them.
        if let Some(ref mut tp) = new_text_pages_locked { 
            tp.remap(page_table, TEXT_SECTION_FLAGS)?;
        }
        if let Some(ref mut rp) = new_rodata_pages_locked { 
            rp.remap(page_table, RODATA_SECTION_FLAGS)?;
        }
        // data/bss sections are already mapped properly, since they're writable

        // set the new_crate's `sections` list, since we didn't do it earlier
        {
            let mut new_crate_mut = new_crate.lock_as_mut()
                .ok_or_else(|| "BUG: LoadedCrate::deep_copy(): couldn't get exclusive mutable access to newly-copied crate")?;
            new_crate_mut.sections = new_sections;
        }

        Ok(new_crate)
    }
}


pub const TEXT_SECTION_NAME            : &str = ".text";
pub const RODATA_SECTION_NAME          : &str = ".rodata";
pub const DATA_SECTION_NAME            : &str = ".data";
pub const BSS_SECTION_NAME             : &str = ".bss";
pub const TLS_DATA_SECTION_NAME        : &str = ".tdata";
pub const TLS_BSS_SECTION_NAME         : &str = ".tbss";
pub const GCC_EXCEPT_TABLE_SECTION_NAME: &str = ".gcc_except_table";
pub const EH_FRAME_SECTION_NAME        : &str = ".eh_frame";


/// The possible types of sections that can be loaded from a crate object file.
#[derive(Debug, Copy, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum SectionType {
    /// A `text` section contains executable code, i.e., functions. 
    Text,
    /// An `rodata` section contains read-only data, i.e., constants.
    Rodata,
    /// A `data` section contains data that is both readable and writable, i.e., static variables. 
    Data,
    /// A `bss` section is just like a data section, but is automatically initialized to all zeroes at load time.
    Bss,
    /// A `.tdata` section is a read-only section that holds the initial data "image" 
    /// for a thread-local storage (TLS) area.
    TlsData,
    /// A `.tbss` section is a read-only section that holds all-zero data for a thread-local storage (TLS) area.
    /// This is is effectively an empty placeholder: the all-zero data section doesn't actually exist in memory.
    TlsBss,
    /// A `.gcc_except_table` section contains landing pads for exception handling,
    /// comprising the LSDA (Language Specific Data Area),
    /// which is effectively used to determine when we should stop the stack unwinding process
    /// (e.g., "catching" an exception). 
    /// 
    /// Blog post from author of gold linker: <https://www.airs.com/blog/archives/464>
    /// 
    /// Mailing list discussion here: <https://gcc.gnu.org/ml/gcc-help/2010-09/msg00116.html>
    /// 
    /// Here is a sample repository parsing this section: <https://github.com/nest-leonlee/gcc_except_table>
    /// 
    GccExceptTable,
    /// The `.eh_frame` section contains information about stack unwinding and destructor functions
    /// that should be called when traversing up the stack for cleanup. 
    /// 
    /// Blog post from author of gold linker: <https://www.airs.com/blog/archives/460>
    /// Some documentation here: <https://gcc.gnu.org/wiki/Dwarf2EHNewbiesHowto>
    /// 
    EhFrame,
}
impl SectionType {
    /// Returns the name of this `SectionType` as a [`StrRef`].
    /// 
    /// This is useful for deduplicating section name strings in memory,
    /// as the returned `StrRef` will point back to a single instance 
    /// of that section name string that can be shared across the system.
    pub fn name_str_ref(&self) -> StrRef {
        static TEXT             : Once<StrRef> = Once::new();
        static RODATA           : Once<StrRef> = Once::new();
        static DATA             : Once<StrRef> = Once::new();
        static BSS              : Once<StrRef> = Once::new();
        static TLS_DATA         : Once<StrRef> = Once::new();
        static TLS_BSS          : Once<StrRef> = Once::new();
        static GCC_EXCEPT_TABLE : Once<StrRef> = Once::new();
        static EH_FRAME         : Once<StrRef> = Once::new();

        let init = || StrRef::from(self.name());

        match self {
            Self::Text           => TEXT.call_once(|| init()).clone(),
            Self::Rodata         => RODATA.call_once(|| init()).clone(),
            Self::Data           => DATA.call_once(|| init()).clone(),
            Self::Bss            => BSS.call_once(|| init()).clone(),
            Self::TlsData        => TLS_DATA.call_once(|| init()).clone(),
            Self::TlsBss         => TLS_BSS.call_once(|| init()).clone(),
            Self::GccExceptTable => GCC_EXCEPT_TABLE.call_once(|| init()).clone(),
            Self::EhFrame        => EH_FRAME.call_once(|| init()).clone(),
        }
    }
    
    /// Returns the const `&str` name of this `SectionType`.
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Text           => TEXT_SECTION_NAME,
            Self::Rodata         => RODATA_SECTION_NAME,
            Self::Data           => DATA_SECTION_NAME, 
            Self::Bss            => BSS_SECTION_NAME,
            Self::TlsData        => TLS_DATA_SECTION_NAME,
            Self::TlsBss         => TLS_BSS_SECTION_NAME,
            Self::GccExceptTable => GCC_EXCEPT_TABLE_SECTION_NAME,
            Self::EhFrame        => EH_FRAME_SECTION_NAME,
        }
    } 

    /// Returns `true` if `Data` or `Bss`, otherwise `false`.
    pub fn is_data_or_bss(&self) -> bool {
        match self {
            Self::Data | Self::Bss => true,
            _ => false,
        }
    }
}

/// The parts of a `LoadedSection` that may be mutable, i.e., 
/// only the parts that could change after a section is initially loaded and linked.
#[derive(Default)]
pub struct LoadedSectionInner {
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
    #[cfg(internal_deps)]
    pub internal_dependencies: Vec<InternalDependency>,
}

/// Represents a section that has been loaded and is part of a `LoadedCrate`.
/// The containing `SectionType` enum determines which type of section it is.
pub struct LoadedSection {
    /// The full string name of this section, a fully-qualified symbol, 
    /// with the format `<crate>::[<module>::][<struct>::]<fn_name>::<hash>`.
    /// The unique hash is generated for each section by the Rust compiler,
    /// which can be used as a version identifier. 
    /// Not all symbols will have a hash, e.g., ones that are not mangled.
    /// 
    /// # Examples
    /// * `test_lib::MyStruct::new::h843a613894da0c24`
    /// * `my_crate::my_function::hbce878984534ceda`   
    pub name: StrRef,
    /// The type of this section, e.g., `.text`, `.rodata`, `.data`, `.bss`, etc.
    pub typ: SectionType,
    /// Whether or not this section's symbol was exported globally (is public)
    pub global: bool,
    /// The `MappedPages` that cover this section.
    pub mapped_pages: Arc<Mutex<MappedPages>>, 
    /// The offset into the `mapped_pages` where this section starts
    pub mapped_pages_offset: usize,
    /// The range of `VirtualAddress`es covered by this section, i.e., 
    /// the starting (inclusive) and ending (exclusive) `VirtualAddress` of this section.
    /// This can be used to calculate size, but is primarily a performance optimization
    /// so we can avoid locking this section's `MappedPages` and avoid recalculating 
    /// its bounds based on its offset and size. 
    /// 
    /// For TLS sections, this `address_range.start` holds the offset (from the TLS base)
    /// into the TLS area where this section's data exists.
    pub address_range: Range<VirtualAddress>, 
    /// The `LoadedCrate` object that contains/owns this section
    pub parent_crate: WeakCrateRef,
    /// The inner contents of a section that could possibly change
    /// after the section was initially loaded and linked. 
    pub inner: RwLock<LoadedSectionInner>,
}
impl LoadedSection {
    /// Create a new `LoadedSection`, with an empty `dependencies` list.
    pub fn new(
        typ: SectionType, 
        name: StrRef, 
        mapped_pages: Arc<Mutex<MappedPages>>,
        mapped_pages_offset: usize,
        virt_addr: VirtualAddress,
        size: usize,
        global: bool, 
        parent_crate: WeakCrateRef,
    ) -> LoadedSection {
        LoadedSection::with_dependencies(
            typ,
            name,
            mapped_pages,
            mapped_pages_offset,
            virt_addr,
            size,
            global,
            parent_crate,
            Default::default(),
            Default::default(),
            #[cfg(internal_deps)]
            Default::default(),
        )
    }

    /// Same as [new()`](#method.new), but uses the given `dependencies` instead of the default empty list.
    pub fn with_dependencies(
        typ: SectionType, 
        name: StrRef, 
        mapped_pages: Arc<Mutex<MappedPages>>,
        mapped_pages_offset: usize,
        virt_addr: VirtualAddress,
        size: usize,
        global: bool, 
        parent_crate: WeakCrateRef,
        sections_i_depend_on: Vec<StrongDependency>,
        sections_dependent_on_me: Vec<WeakDependent>,
        #[cfg(internal_deps)]
        internal_dependencies: Vec<InternalDependency>,
    ) -> LoadedSection {
        LoadedSection {
            typ,
            name,
            mapped_pages,
            mapped_pages_offset,
            address_range: virt_addr .. (virt_addr + size),
            global,
            parent_crate,
            inner: RwLock::new(LoadedSectionInner {
                sections_i_depend_on,
                sections_dependent_on_me,
                #[cfg(internal_deps)]
                internal_dependencies,
            }),
        }
    }

    /// Returns the starting `VirtualAddress` of where this section is loaded into memory. 
    pub fn start_address(&self) -> VirtualAddress {
        self.address_range.start
    }

    /// Returns the size in bytes of this section.
    pub fn size(&self) -> usize {
        self.address_range.end.value() - self.address_range.start.value()
    }

    /// Returns the type of this section.
    pub fn get_type(&self) -> SectionType {
        self.typ
    }

    /// Returns the substring of this section's name that excludes the trailing hash. 
    /// 
    /// See the identical associated function [`section_name_without_hash()`](#fn.section_name_without_hash.html) for more. 
    pub fn name_without_hash(&self) -> &str {
        Self::section_name_without_hash(self.name.as_str())
    }


    /// Returns the substring of the given section's name that excludes the trailing hash,
    /// but includes the hash delimiter "`::h`". 
    /// If there is no hash, then it returns the full section name unchanged.
    /// 
    /// # Examples
    /// name: "`keyboard_new::init::h832430094f98e56b`", return value: "`keyboard_new::init::h`"
    /// name: "`start_me`", return value: "`start_me`"
    pub fn section_name_without_hash(sec_name: &str) -> &str {
        sec_name.rfind(SECTION_HASH_DELIMITER)
            .and_then(|end| sec_name.get(0 .. (end + SECTION_HASH_DELIMITER.len())))
            .unwrap_or_else(|| &sec_name)
    }


    /// Returns the index of the first `WeakDependent` object in this `LoadedSection`'s `sections_dependent_on_me` list
    /// in which the section matches the given `matching_section` 
    pub fn find_weak_dependent(&self, matching_section: &StrongSectionRef) -> Option<usize> {
        for (index, weak_dep) in self.inner.read().sections_dependent_on_me.iter().enumerate() {
            if let Some(sec) = weak_dep.section.upgrade() {
                if Arc::ptr_eq(matching_section, &sec) {
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
    pub fn copy_section_data_to(&self, destination_section: &LoadedSection) -> Result<(), &'static str> {

        let mut dest_sec_mapped_pages = destination_section.mapped_pages.lock();
        let dest_sec_data: &mut [u8] = dest_sec_mapped_pages.as_slice_mut(destination_section.mapped_pages_offset, destination_section.size())?;

        let source_sec_mapped_pages = self.mapped_pages.lock();
        let source_sec_data: &[u8] = source_sec_mapped_pages.as_slice(self.mapped_pages_offset, self.size())?;

        if dest_sec_data.len() == source_sec_data.len() {
            dest_sec_data.copy_from_slice(source_sec_data);
            // debug!("Copied data from source section {:?} {:?} ({:#X}) to dest section {:?} {:?} ({:#X})",
            //     self.typ, self.name, self.size(), destination_section.typ, destination_section.name, destination_section.size());
            Ok(())
        }
        else {
            error!("This source section {:?}'s size ({:#X}) is different from the destination section {:?}'s size ({:#X})",
                self.name, self.size(), destination_section.name, destination_section.size());
            Err("this source section has a different length than the destination section")
        }
    }

    /// Reinterprets this section's underlying `MappedPages` memory region as an executable function.
    ///
    /// The generic `F` parameter is the function type signature itself, e.g., `fn(String) -> u8`.
    /// 
    /// Returns a reference to the function that is formed from the underlying memory region,
    /// with a lifetime dependent upon the lifetime of this section.
    ///
    /// # Safety
    /// The type signature of `F` must match the type signature of the function.
    ///
    /// # Locking
    /// Obtains the lock on this section's `MappedPages` object.
    ///
    /// # Note
    /// Ideally, we would use debug information to know the size of the entire function
    /// and test whether that fits within the bounds of the memory region, rather than just checking
    /// the size of `F`, the function pointer/signature.
    /// Without debug information, checking the size is restricted to in-bounds memory safety 
    /// rather than actual functional correctness. 
    ///
    /// # Examples
    /// Here's how you might call this function:
    /// ```
    /// type MyPrintFuncSignature = fn(&str) -> Result<(), &'static str>;
    /// let section = mod_mgmt::get_symbol_starting_with("my_crate::print::").upgrade().unwrap();
    /// let print_func: &MyPrintFuncSignature = unsafe { section.as_func() }.unwrap();
    /// print_func("hello there");
    /// ```
    /// 
    pub unsafe fn as_func<F>(&self) -> Result<&F, &'static str> {
        if false {
            debug!("Requested LoadedSection {:#X?} as function {:?}", self, core::any::type_name::<F>());
        }

        let mp = self.mapped_pages.lock();
        // Check flags to make sure these pages are executable (otherwise a page fault would occur when this func is called)
        if self.typ != SectionType::Text || !mp.flags().is_executable() {
            error!("Requested LoadedSection as function {:?}, but was not an executable text section! (flags: {:?})",
                core::any::type_name::<F>(), mp.flags()
            );
            return Err("as_func(): section was not an executable text section");
        }

        // Check that the bounds of this entire section fit within its MappedPages
        let end = self.mapped_pages_offset + self.size();
        if end > mp.size_in_bytes() {
            error!("Requested LoadedSection as function {:?}, but section's end offset ({:X?}) was beyond its MappedPages ({:X?})",
                core::any::type_name::<F>(), end, mp.size_in_bytes()
            );
            return Err("requested type and offset would not fit within the MappedPages bounds");
        }

        // SAFETY: We checked the section type, executability, and size bounds of the
        // underlying MappedPages above. The lifetime of the returned function
        // reference is tied to this section's lifetime. The caller guarantees
        // that the function signature matches.
        Ok(unsafe { 
            core::mem::transmute(
                &(mp.start_address().value() + self.mapped_pages_offset)
            )
        })
    }
}

impl fmt::Display for LoadedSection {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "LoadedSection({:?}, typ: {:?}, vaddr: {:#X}, size: {})", 
            self.name,
            self.typ,
            self.start_address(),
            self.size(),
        )
    }
}

impl fmt::Debug for LoadedSection {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut dbg = f.debug_struct("LoadedSection");
        dbg.field("name", &self.name);
        dbg.field("typ", &self.typ);

        // Try to get the parent_crate's name
        if let Some(parent_crate_name) = self.parent_crate
            .upgrade()
            .and_then(|cref| 
                cref.try_lock_as_ref()
                    .map(|c| c.crate_name.clone())
            )
        {
            dbg.field("parent", &parent_crate_name);
        }
        else {
            dbg.field("parent", &"<locked>");
        }

        // Add the rest of the typical fields
        dbg.field("vaddr", &self.start_address())
            .field("size", &self.size())
            .finish_non_exhaustive()
    }
}


/// A representation that the owner `A` of (a `LoadedSection` object containing) this struct
/// depends on the given `section` `B` in this struct.
/// The dependent section `A` is not specifically included here;
/// since it's the owner of this struct, it's implicit that it's the dependent one.
///  
/// A dependency is a strong reference to another `LoadedSection` `B`,
/// because that other section `B` shouldn't be removed as long as there are still sections (`A`) that depend on it.
/// 
/// This is the inverse of the [`WeakDependency`](#struct.WeakDependency) type.
#[derive(Debug, Clone)]
pub struct StrongDependency {
    /// A strong reference to the `LoadedSection` `B` that the owner of this struct (`A`) depends on.
    pub section: StrongSectionRef,
    /// The details of the relocation action that was performed.
    pub relocation: RelocationEntry,
}


/// A representation that the `section` `A` in this struct
/// depends on the owner `B` of (the `LoadedSection` object containing) this struct. 
/// The target dependency `B` is not specifically included here; 
/// it's implicitly the owner of this struct.
///  
/// This is a weak reference to another `LoadedSection` `A`,
/// because it is okay to remove a section `A` that depends on the owning section `B` before removing `B`.
/// Otherwise, there would be an infinitely recursive dependency, and neither `A` nor `B` could ever be dropped.
/// This design allows for `A` to be dropped before `B`, because there is no dependency ordering violation there.
/// 
/// This is the inverse of the [`StrongDependency`](#struct.StrongDependency) type.
#[derive(Debug, Clone)]
pub struct WeakDependent {
    /// A weak reference to the `LoadedSection` `A` that depends on the owner `B` of this struct.
    pub section: WeakSectionRef,
    /// The details of the relocation action that was performed.
    pub relocation: RelocationEntry,
}


/// The information necessary to calculate and write a relocation value,
/// based on a source section and a target section, in which a value 
/// based on the location of the source section is written somwhere in the target section.
#[derive(Copy, Clone, Eq, PartialEq)]
pub struct RelocationEntry {
    /// The type of relocation calculation that is performed 
    /// to connect the target section to the source section.
    pub typ: u32,
    /// The value that is added to the source section's address 
    /// when performing the calculation of the source value that is written to the target section.
    pub addend: usize,
    /// The offset from the starting virtual address of the target section
    /// that specifies where the relocation value should be written.
    pub offset: usize,
}

impl fmt::Debug for RelocationEntry {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "RelocationEntry {{ type: {:#X}, addend: {:#X}, offset: {:#X} }}",
            self.typ, self.addend, self.offset
        )
    }
}

impl RelocationEntry {
    pub fn from_elf_relocation(rela_entry: &xmas_elf::sections::Rela<u64>) -> RelocationEntry {
        RelocationEntry {
            typ: rela_entry.get_type(),
            addend: rela_entry.get_addend() as usize,
            offset: rela_entry.get_offset() as usize,
        }
    }

    /// Returns true if the relocation type results in a relocation calculation
    /// in which the source value written into the target section 
    /// does NOT depend on the target section's address itself in any way 
    /// (i.e., it only depends on the source section)
    pub fn is_absolute(&self) -> bool {
        match self.typ {
            R_X86_64_32 | 
            R_X86_64_64 => true,
            _ => false,
        }
    }
}


/// A representation that the section that owns this struct 
/// has a dependency on the given `source_sec`, *in the same crate*.
/// The dependency itself is specified via the other section's shndx.
#[cfg(internal_deps)]
#[derive(Debug, Clone)]
pub struct InternalDependency {
    pub relocation: RelocationEntry,
    pub source_sec_shndx: Shndx,
}
#[cfg(internal_deps)]
impl InternalDependency {
    pub fn new(relocation: RelocationEntry, source_sec_shndx: Shndx) -> InternalDependency {
        InternalDependency {
            relocation, source_sec_shndx
        }
    }
}


/// Write an actual relocation entry.
/// # Arguments
/// * `relocation_entry`: the relocation entry from the ELF file that specifies the details of the relocation action to perform.
/// * `target_sec_mapped_pages`: the `MappedPages` that covers the target section, i.e., the section where the relocation data will be written to.
/// * `target_sec_mapped_pages_offset`: the offset into `target_sec_mapped_pages` where the target section is located.
/// * `source_sec_vaddr`: the `VirtualAddress` of the source section of the relocation, i.e., the section that the `target_sec` depends on and "points" to.
/// * `verbose_log`: whether to output verbose logging information about this relocation action.
pub fn write_relocation(
    relocation_entry: RelocationEntry,
    target_sec_mapped_pages: &mut MappedPages,
    target_sec_mapped_pages_offset: usize,
    source_sec_vaddr: VirtualAddress,
    verbose_log: bool
) -> Result<(), &'static str> {
    // Calculate exactly where we should write the relocation data to.
    let target_offset = target_sec_mapped_pages_offset + relocation_entry.offset;

    // Perform the actual relocation data writing here.
    // There is a great, succint table of relocation types here
    // https://docs.rs/goblin/0.0.24/goblin/elf/reloc/index.html
    match relocation_entry.typ {
        R_X86_64_32 => {
            let target_ref: &mut u32 = target_sec_mapped_pages.as_type_mut(target_offset)?;
            let source_val = source_sec_vaddr.value().wrapping_add(relocation_entry.addend);
            if verbose_log { trace!("                    target_ptr: {:#X}, source_val: {:#X} (from source_sec_vaddr {:#X})", target_ref as *mut _ as usize, source_val, source_sec_vaddr); }
            *target_ref = source_val as u32;
        }
        R_X86_64_64 => {
            let target_ref: &mut u64 = target_sec_mapped_pages.as_type_mut(target_offset)?;
            let source_val = source_sec_vaddr.value().wrapping_add(relocation_entry.addend);
            if verbose_log { trace!("                    target_ptr: {:#X}, source_val: {:#X} (from source_sec_vaddr {:#X})", target_ref as *mut _ as usize, source_val, source_sec_vaddr); }
            *target_ref = source_val as u64;
        }
        R_X86_64_PC32 |
        R_X86_64_PLT32 => {
            let target_ref: &mut u32 = target_sec_mapped_pages.as_type_mut(target_offset)?;
            let source_val = source_sec_vaddr.value().wrapping_add(relocation_entry.addend).wrapping_sub(target_ref as *mut _ as usize);
            if verbose_log { trace!("                    target_ptr: {:#X}, source_val: {:#X} (from source_sec_vaddr {:#X})", target_ref as *mut _ as usize, source_val, source_sec_vaddr); }
            *target_ref = source_val as u32;
        }
        R_X86_64_PC64 => {
            let target_ref: &mut u64 = target_sec_mapped_pages.as_type_mut(target_offset)?;
            let source_val = source_sec_vaddr.value().wrapping_add(relocation_entry.addend).wrapping_sub(target_ref as *mut _ as usize);
            if verbose_log { trace!("                    target_ptr: {:#X}, source_val: {:#X} (from source_sec_vaddr {:#X})", target_ref as *mut _ as usize, source_val, source_sec_vaddr); }
            *target_ref = source_val as u64;
        }
        R_X86_64_TPOFF32 => {
            let target_ref: &mut u32 = target_sec_mapped_pages.as_type_mut(target_offset)?;
            let source_val = u32::try_from(source_sec_vaddr.value())
                .map_err(|_| "BUG: TLS relocation (R_X86_64_TPOFF32) source section value (TLS offset) cannot fit in a `u32`")?;
            if verbose_log { trace!("                    target_ptr: {:#X}, source_val: {:#X} (from source_sec_vaddr {:#X})", target_ref as *mut _ as usize, source_val, source_sec_vaddr); }
            *target_ref = source_val;
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
