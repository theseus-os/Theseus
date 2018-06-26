//! This metadata module contains metadata about all other modules/crates loaded in Theseus.
//! 
//! [This is a good link](https://users.rust-lang.org/t/circular-reference-issue/9097)
//! for understanding why we need `Arc`/`Weak` to handle recursive/circular data structures in Rust. 

use core::ops::{Deref};
use spin::{Mutex, RwLock};
use alloc::{Vec, String, BTreeMap};
use alloc::arc::{Arc, Weak};
use memory::MappedPages;
use dependency::*;

use super::SymbolMap;

/// A Strong reference (`Arc`) to a `LoadedSection`.
pub type StrongSectionRef  = Arc<Mutex<LoadedSection>>;
/// A Weak reference (`Weak`) to a `LoadedSection`.
pub type WeakSectionRef = Weak<Mutex<LoadedSection>>;
/// A Strong reference (`Arc`) to a `LoadedCrate`.
pub type StrongCrateRef  = Arc<RwLock<LoadedCrate>>;
/// A Weak reference (`Weak`) to a `LoadedCrate`.
pub type WeakCrateRef = Weak<RwLock<LoadedCrate>>;


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
    /// The name of this crate.
    pub crate_name: String,
    /// The list of all sections in this crate.
    pub sections: Vec<StrongSectionRef>,
    /// The `MappedPages` that include the text sections for this crate,
    /// i.e., sections that are readable and executable, but not writable.
    pub text_pages: Option<MappedPages>, //Option<Arc<Mutex<MappedPages>>>,
    /// The `MappedPages` that include the rodata sections for this crate.
    /// i.e., sections that are read-only, not writable nor executable.
    pub rodata_pages: Option<MappedPages>, //Option<Arc<Mutex<MappedPages>>>,
    /// The `MappedPages` that include the data and bss sections for this crate.
    /// i.e., sections that are readable and writable but not executable.
    pub data_pages: Option<MappedPages>, //Option<Arc<Mutex<MappedPages>>>,

    // crate_dependencies: Vec<LoadedCrate>,
}

use core::fmt;
impl fmt::Debug for LoadedCrate {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "LoadedCrate{{{}}}", self.crate_name)
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
    pub fn get_function_section(&self, func_name: &str) -> Option<StrongSectionRef> {
        self.find_section(|sec| sec.is_text() && sec.name == func_name)
    }

    /// Returns the first `LoadedSection` that matches the given predicate
    pub fn find_section<F>(&self, predicate: F) -> Option<StrongSectionRef> 
        where F: Fn(&LoadedSection) -> bool
    {
        self.sections.iter().filter(|sec_ref| {
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

        for sec in &self.sections {
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
#[derive(Debug, PartialEq)]
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
    /// The offset into the `parent_crate`'s `MappedPages` where this section starts
    pub mapped_pages_offset: usize,
    /// The size in bytes of this section
    pub size: usize,
    /// Whether or not this section's symbol was exported globally (is public)
    pub global: bool,
    /// The `LoadedCrate` object that contains/owns this section
    pub parent_crate: WeakCrateRef,
    /// The list of other sections that this section depends on, i.e., "my required dependencies".
    /// This is kept as a list of strong references because these sections must outlast this section,
    /// i.e., those sections cannot be removed/deleted until this one is deleted.
    pub sections_i_depend_on: Vec<StrongDependency>,
    /// The list of other sections that depend on this section, i.e., "my dependents".
    /// This is kept as a list of Weak references because we must be able to remove other sections
    /// that are dependent upon this one before we remove this one.
    /// If we kept strong references to the sections dependent on this one, 
    /// then we wouldn't be able to remove/delete those sections before deleting this one.
    pub sections_dependent_on_me: Vec<WeakDependent>,
}
impl LoadedSection {
    /// Create a new `LoadedSection`, with an empty `dependencies` list.
    pub fn new(
        typ: SectionType, 
        name: String, 
        hash: Option<String>, 
        mapped_pages_offset: usize,
        size: usize,
        global: bool, 
        parent_crate: WeakCrateRef
    ) -> LoadedSection {
        LoadedSection::with_dependencies(typ, name, hash, mapped_pages_offset, size, global, parent_crate, Vec::new(), Vec::new())
    }

    /// Same as `LoadedSection::new()`, but uses the given `dependencies` instead of the default empty list.
    pub fn with_dependencies(
        typ: SectionType, 
        name: String, 
        hash: Option<String>, 
        mapped_pages_offset: usize,
        size: usize,
        global: bool, 
        parent_crate: WeakCrateRef,
        sections_i_depend_on: Vec<StrongDependency>,
        sections_dependent_on_me: Vec<WeakDependent>,
    ) -> LoadedSection {
        LoadedSection {
            typ, name, hash, mapped_pages_offset, size, global, parent_crate, sections_i_depend_on, sections_dependent_on_me
        }
    }

    /// Returns a reference to the `MappedPages` that covers this `LoadedSection`.
    /// Because that `MappedPages` object is owned by this `LoadedSection`'s `parent_crate`,
    /// the lifetime of the returned `MappedPages` reference is tied to the lifetime 
    /// of the given `LoadedCrate` parent crate object.
    /// The given `parent_crate` reference should be obtained by invoking 
    /// the [`parent_crate()`](#method.parent_crate) method. 
    pub fn mapped_pages<'a>(&self, parent_crate: &'a LoadedCrate) -> Option<&'a MappedPages> {
        match self.typ {
            SectionType::Text   => parent_crate.text_pages.as_ref(),
            SectionType::Rodata => parent_crate.rodata_pages.as_ref(),
            SectionType::Data |
            SectionType::Bss    => parent_crate.data_pages.as_ref(),
        }
    }

    /// Returns a mutable reference to the `MappedPages` that covers this `LoadedSection`.
    /// Because that `MappedPages` object is owned by this `LoadedSection`'s `parent_crate`,
    /// the lifetime of the returned `MappedPages` reference is tied to the lifetime 
    /// of the given `LoadedCrate` parent crate object. 
    /// The given `parent_crate` reference should be obtained by invoking 
    /// the [`parent_crate_mut()`](#method.parent_crate_mut) method.
    pub fn mapped_pages_mut<'a>(&self, parent_crate: &'a mut LoadedCrate) -> Option<&'a mut MappedPages> {
        match self.typ {
            SectionType::Text   => parent_crate.text_pages.as_mut(),
            SectionType::Rodata => parent_crate.rodata_pages.as_mut(),
            SectionType::Data |
            SectionType::Bss    => parent_crate.data_pages.as_mut(),
        }
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
    pub (crate) fn copy_section_data_to(&self, 
        this_section_parent_crate: &LoadedCrate, 
        destination_section: &mut LoadedSection,
        destination_section_parent_crate: &mut LoadedCrate
    ) -> Result<(), &'static str> {

        let dest_sec_mapped_pages = self.mapped_pages_mut(destination_section_parent_crate).ok_or("Couldn't get the destination_section's MappedPages")?;
        let dest_sec_data: &mut [u8] = dest_sec_mapped_pages.as_slice_mut(destination_section.mapped_pages_offset, destination_section.size)?;

        let source_sec_mapped_pages = self.mapped_pages(this_section_parent_crate).ok_or("Couldn't get this section's MappedPages")?;
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
