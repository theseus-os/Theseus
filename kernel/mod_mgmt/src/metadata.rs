//! This metadata module contains metadata about all other modules/crates loaded in Theseus.
//! 
//! [This is a good link](https://users.rust-lang.org/t/circular-reference-issue/9097)
//! for understanding why we need `Arc`/`Weak` to handle recursive/circular data structures in Rust. 

use core::ops::{Deref, DerefMut};
use spin::{Mutex, RwLock, RwLockReadGuard, RwLockWriteGuard};
use alloc::{Vec, String, BTreeMap};
use alloc::arc::{Arc, Weak};
use memory::{MappedPages, VirtualMemoryArea};

use owning_ref::{ArcRef, OwningRef, RwLockReadGuardRef, RwLockWriteGuardRefMut};
use namespace::*;



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



pub struct ElfProgramSegment {
    /// the VirtualMemoryAddress that will represent the virtual mapping of this Program segment.
    /// Provides starting virtual address, size in memory, mapping flags, and a text description.
    pub vma: VirtualMemoryArea,
    /// the offset of this segment into the file.
    /// This plus the physical address of the Elf file is the physical address of this Program segment.
    pub offset: usize,
}




/// Represents a single crate object file that has been loaded into the system.
#[derive(Debug)]
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

impl LoadedCrate {
    /// Returns the `LoadedSection` of type `SectionType::Text` that matches the requested function name, if it exists in this `LoadedCrate`.
    /// Only matches demangled names, e.g., "my_crate::foo".
    pub fn get_function_section(&self, func_name: &str) -> Option<StrongSectionRef> {
        self.sections.iter().filter(|sec_ref| {
            let sec = sec_ref.lock();
            sec.is_text() && sec.name == func_name
        }).next().cloned()
    }

    /// Returns a map containing all of this crate's global symbols 
    pub fn global_symbol_map(&self) -> SymbolMap {
        self.symbol_map(|sec| sec.global)
    }

    /// Returns a map containing all of this crate's symbols,
    /// filtered to include only `LoadedSection`s that satisfy the given predicate
    /// (if the predicate returns true for a given section, then it is included in the map).
    pub fn symbol_map<F>(&self, predicate: F) -> SymbolMap 
        where F: Fn(&LoadedSection) -> bool
    {
        let mut map: SymbolMap = BTreeMap::new();
        for sec in self.sections.iter().filter(|sec| predicate(sec.lock().deref())) {
            let key = sec.lock().name.clone();
            if let Some(old_val) = map.insert(key.clone(), Arc::downgrade(&sec)) {
                if key.ends_with("_LOC") || self.crate_name == "nano_core" {
                    // ignoring these special cases currently
                }
                else {
                    warn!("symbol_map(): crate \"{}\" had duplicate section for symbol \"{}\", old: {:?}, new: {:?}", 
                        self.crate_name, key, old_val.upgrade(), sec);
                }
            }
        }

        map
    }
}


/// The possible types of `LoadedSection`s: .text, .rodata, or .data.
/// A .bss section is considered the same as .data.
#[derive(Debug, PartialEq)]
pub enum SectionType {
    Text, 
    Rodata,
    Data,
}

/// Represents a .text, .rodata, .data, or .bss section
/// that has been loaded and is part of a `LoadedCrate`.
/// The containing `SectionType` enum determines which type of section it is.
#[derive(Debug)]
pub struct LoadedSection {
    /// The type of this section: .text, .rodata, or .data.
    /// A .bss section is considered the same as .data.
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
    /// The sections that this section depends on.
    /// This is kept as a list of strong references because these dependency sections must outlast this section,
    /// i.e., those sections cannot be removed/deleted until this one is deleted.
    pub dependencies: Vec<RelocationDependency>,
    // /// The sections that depend on this section. 
    // /// This is kept as a list of Weak references because we must be able to remove other sections
    // /// that are dependent upon this one before we remove this one.
    // /// If we kept strong references to the sections dependent on this one, 
    // /// then we wouldn't be able to remove/delete those sections before deleting this one.
    // pub dependents: Vec<WeakSectionRef>,
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
        LoadedSection::with_dependencies(typ, name, hash, mapped_pages_offset, size, global, parent_crate, Vec::new())
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
        dependencies: Vec<RelocationDependency>
    ) -> LoadedSection {
        LoadedSection {
            typ, name, hash, mapped_pages_offset, size, global, parent_crate, dependencies
        }
    }


    /// Obtains a read-only lock to to this section's parent `LoadedCrate`,
    /// if it exists, and returns an immutable reference to it.
    pub fn parent_crate<'a>(&'a self) -> Option<OwningRef<StrongCrateRef, LoadedCrate>> {

        let oref = ArcRef::new(self.parent_crate.upgrade().unwrap());

            // .map(|pc_arc| pc_arc.deref());

        
        // RwLockReadGuardRef::new()
        // let r = OwningRef::new()
        //     .map(|pc_arc| {
        //         OwningRef::new(pc_arc).map(|a| a.read()) 
        //     });


        None
    }

    /// Obtains a writable lock to to this section's parent `LoadedCrate`,
    /// if it exists, and returns a mutable reference to it.
    pub fn parent_crate_mut(&self) -> Option<OwningRef<StrongCrateRef, LoadedCrate>> {
        // self.parent_crate.upgrade().map(|pc_arc| {
        //     OwningRef::new(pc_arc).map(|a| a.write().deref_mut()) 
        // })
        None
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
            SectionType::Data   => parent_crate.data_pages.as_ref(),
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
            SectionType::Data   => parent_crate.data_pages.as_mut(),
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

    /// Whether this `LoadedSection` is a .data or .bss section
    pub fn is_data_or_bss(&self) -> bool {
        self.typ == SectionType::Data
    }
}


/// A representation that the section object containing this struct
/// has a dependency on the given `section`.
/// The dependent section is not specifically included here;
/// it's implicit that the owner of this object is the one who depends on the `section`.
///  
/// A dependency is a strong reference to another `LoadedSection`,
/// because a given section should not be removed if there are still sections that depend on it.
#[derive(Debug)]
pub struct RelocationDependency {
    pub section: StrongSectionRef,
    pub rel_type: u32,
    pub offset: usize,
}
