//! This metadata module contains metadata about all other modules/crates loaded in Theseus.
//! 
//! [This is a good link](https://users.rust-lang.org/t/circular-reference-issue/9097)
//! for understanding why we need `Arc`/`Weak` to handle recursive/circular data structures in Rust. 

use core::ops::{Deref};
use spin::{Mutex, RwLock};
use alloc::{Vec, String, BTreeMap};
use alloc::arc::{Arc, Weak};
use alloc::btree_map::Entry; 
use memory::{MappedPages, MemoryManagementInfo, get_module, VirtualMemoryArea};

lazy_static! {
    /// The main metadata structure that contains a tree of all loaded crates.
    /// Maps a String crate_name to its crate instance.
    static ref CRATE_TREE: Mutex<BTreeMap<String, StrongCrateRef>> = Mutex::new(BTreeMap::new());
}


lazy_static! {
    /// A flat map of all symbols currently loaded into the kernel. 
    /// Maps a fully-qualified kernel symbol name (String) to the corresponding `LoadedSection`. 
    /// Symbols declared as "no_mangle" will appear in the root namespace with no crate prefixex, as expected.
    static ref SYSTEM_MAP: Mutex<SymbolMap> = Mutex::new(BTreeMap::new());
}




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




/// Simple debugging function for outputting the system map.
pub fn dump_symbol_map() -> String {
    use core::fmt::Write;
    let mut output: String = String::new();
    let sysmap = SYSTEM_MAP.lock();
    match write!(&mut output, "{:?}", sysmap.keys().collect::<Vec<&String>>()) {
        Ok(_) => output,
        _ => String::from("error"),
    }
}


/// Adds a new crate to the module tree, and adds only its global symbols to the system map.
/// Returns the number of *new* unique global symbols added to the system map. 
/// If a symbol already exists in the system map, this leaves it intact and *does not* replace it.
pub fn add_crate(new_crate: StrongCrateRef, log_replacements: bool) -> usize {
    let crate_name = new_crate.read().crate_name.clone();
    let mut system_map = SYSTEM_MAP.lock();
    let new_map = new_crate.read().global_symbol_map();

    // We *could* just use `append()` here, but that wouldn't let us know which entries
    // in the system map were being overwritten, which is currently a valuable bit of debugging info that we need.
    // Proper way for the future:  system_map.append(&mut new_map);

    // add all the global symbols to the system map, in a way that lets us inspect/log each one
    let mut count = 0;
    for (key, new_sec) in new_map {
        // instead of blindly replacing old symbols with their new version, we leave all old versions intact 
        // TODO NOT SURE IF THIS IS THE CORRECT WAY, but blindly replacing them all is definitely wrong
        // The correct way is probably to use the hash values to disambiguate, but then we have to ensure deterministic/persistent hashes across different compilations
        let entry = system_map.entry(key.clone());
        match entry {
            Entry::Occupied(old_val) => {
                if let (Some(new_sec), Some(old_sec)) = (new_sec.upgrade(), old_val.get().upgrade()) {
                    let new_sec_size = new_sec.lock().size;
                    let old_sec_size = old_sec.lock().size;
                    if old_sec_size == new_sec_size {
                        if log_replacements { info!("       add_crate \"{}\": Ignoring new symbol already present: {}", crate_name, key); }
                    }
                    else {
                        if log_replacements { 
                            warn!("       add_crate \"{}\": unexpected: different section sizes (old={}, new={}), ignoring new symbol: {}", 
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

    CRATE_TREE.lock().insert(crate_name, new_crate);
    count
}


/// Crate names must be only alphanumeric characters, an underscore, or a dash. 
/// See: <https://www.reddit.com/r/rust/comments/4rlom7/what_characters_are_allowed_in_a_crate_name/>
fn is_valid_crate_name_char(c: char) -> bool {
    char::is_alphanumeric(c) || 
    c == '_' || 
    c == '-'
}

/// A convenince function that returns a weak reference to the `LoadedSection`
/// that matches the given name (`demangled_full_symbol`), if it exists in the system map.
fn get_symbol_internal(demangled_full_symbol: &str) -> Option<WeakSectionRef> {
    SYSTEM_MAP.lock().get(demangled_full_symbol).cloned()
}


/// Finds the corresponding `LoadedSection` reference for the given fully-qualified symbol string.
/// 
/// # Note
/// This is not an interrupt-safe function. DO NOT call it from within an interrupt handler context.
pub fn get_symbol(demangled_full_symbol: &str) -> WeakSectionRef {
    get_symbol_internal(demangled_full_symbol)
        .unwrap_or(Weak::default())
}


/// Finds the corresponding `LoadedSection` reference for the given fully-qualified symbol string,
/// similar to the simpler function `get_symbol()`.
/// 
/// If the symbol cannot be found, it tries to load the kernel crate containing that symbol. 
/// This can only be done for symbols that have a leading crate name, such as "my_crate::foo";
/// if a symbol was given the `no_mangle` attribute, then we will not be able to find it
/// and that symbol's containing crate should be manually loaded before invoking this. 
/// 
/// # Note
/// This is not an interrupt-safe function. DO NOT call it from within an interrupt handler context.
pub fn get_symbol_or_load(demangled_full_symbol: &str, kernel_mmi: &mut MemoryManagementInfo) -> WeakSectionRef {
    if let Some(sec) = get_symbol_internal(demangled_full_symbol) {
        return sec;
    }

    // If we couldn't get the symbol, then we attempt to load the kernel crate containing that symbol.
    // We are only able to do this for mangled symbols, those that have a leading crate name,
    // such as "my_crate::foo". 
    // If "foo()" was marked no_mangle, then we don't know which crate to load. 
    if let Some(crate_dependency_name) = demangled_full_symbol.split("::").next() {
        // Get the last word right before the first "::", which handles symbol names like:
        // <*const T as core::fmt::Debug>::fmt   -->  "core" 
        // <alloc::boxed::Box<T>>::into_unique   -->  "alloc"
        let crate_dependency_name = crate_dependency_name
            .rsplit(|c| !is_valid_crate_name_char(c))
            .next() // the first element of the iterator (last element before the "::")
            .unwrap_or(crate_dependency_name); // if we can't parse it, just stick with the original crate name


        info!("Symbol \"{}\" not initially found, attemping to load its containing crate {:?}", 
            demangled_full_symbol, crate_dependency_name);
        
        // module names have a prefix like "__k_", so we need to prepend that to the crate name
        let crate_dependency_name = format!("{}{}", super::CrateType::Kernel.prefix(), crate_dependency_name);

        if let Some(dependency_module) = get_module(&crate_dependency_name) {
            // try to load the missing symbol's containing crate
            if let Ok(_num_new_syms) = super::load_kernel_crate(dependency_module, kernel_mmi, false) {
                // try again to find the missing symbol
                if let Some(sec) = get_symbol_internal(demangled_full_symbol) {
                    return sec;
                }
                else {
                    error!("Symbol \"{}\" not found, even after loading its containing crate \"{}\". Is that symbol actually in the crate?", 
                        demangled_full_symbol, crate_dependency_name);                                                        
                }
            }
        }
        else {
            error!("Symbol \"{}\" not found, and cannot find module for its containing crate \"{}\".", 
                demangled_full_symbol, crate_dependency_name);
        }
    }
    else {
        error!("Symbol \"{}\" not found, cannot determine its containing crate (no leading crate namespace). Try loading the crate manually first.", 
            demangled_full_symbol);
    }

    // effectively the same as returning None, since it must be upgraded to an Arc before being used
    Weak::default()
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

/// 
pub type SymbolMap = BTreeMap<String, WeakSectionRef>;
/// A Strong reference (`Arc`) to a `LoadedSection`.
pub type StrongSectionRef  = Arc<Mutex<LoadedSection>>;
/// A Weak reference (`Weak`) to a `LoadedSection`.
pub type WeakSectionRef = Weak<Mutex<LoadedSection>>;
/// A Strong reference (`Arc`) to a `LoadedCrate`.
pub type StrongCrateRef  = Arc<RwLock<LoadedCrate>>;
/// A Weak reference (`Weak`) to a `LoadedCrate`.
pub type WeakCrateRef = Weak<RwLock<LoadedCrate>>;


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

    /// Returns a reference to the `MappedPages` that covers this `LoadedSection`.
    /// Because that `MappedPages` object is owned by this `LoadedSection`'s `parent_crate`,
    /// the lifetime of the returned `MappedPages` reference is tied to the lifetime 
    /// of the given `LoadedCrate` parent crate object. 
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
