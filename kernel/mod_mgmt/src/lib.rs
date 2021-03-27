#![no_std]
#![feature(rustc_private)]
#![feature(const_fn)]

#[macro_use] extern crate alloc;
#[macro_use] extern crate log;
extern crate spin;
extern crate xmas_elf;
extern crate memory;
extern crate memory_initialization;
extern crate kernel_config;
extern crate util;
extern crate crate_name_utils;
extern crate crate_metadata;
extern crate rustc_demangle;
extern crate cow_arc;
extern crate qp_trie;
extern crate root;
extern crate vfs_node;
extern crate fs_node;
extern crate path;
extern crate memfs;
extern crate cstr_core;
extern crate hashbrown;

use core::{
    fmt,
    ops::{Deref, Range},
};
use alloc::{
    vec::Vec,
    collections::{BTreeMap, btree_map, BTreeSet},
    string::{String, ToString},
    sync::{Arc, Weak},
};
use spin::{Mutex, Once};
use xmas_elf::{
    ElfFile,
    sections::{SectionData, ShType, SHF_WRITE, SHF_ALLOC, SHF_EXECINSTR},
};
use util::round_up_power_of_two;
use memory::{MmiRef, MemoryManagementInfo, VirtualAddress, MappedPages, EntryFlags, allocate_pages_by_bytes, allocate_frames_by_bytes_at};
use memory_initialization::BootloaderModule;
use cow_arc::CowArc;
use rustc_demangle::demangle;
use qp_trie::{Trie, wrapper::BString};
use fs_node::{FileOrDir, File, FileRef, DirRef};
use vfs_node::VFSDirectory;
use path::Path;
use memfs::MemFile;
use hashbrown::HashMap;
pub use crate_name_utils::{get_containing_crate_name, replace_containing_crate_name, crate_name_from_path};
pub use crate_metadata::*;


pub mod parse_nano_core;
pub mod replace_nano_core_crates;


/// The name of the directory that contains all of the CrateNamespace files.
pub const NAMESPACES_DIRECTORY_NAME: &'static str = "namespaces";

/// The initial `CrateNamespace` that all kernel crates are added to by default.
static INITIAL_KERNEL_NAMESPACE: Once<Arc<CrateNamespace>> = Once::new();

/// Returns a reference to the default kernel namespace, 
/// which must exist because it contains the initially-loaded kernel crates. 
/// Returns None if the default namespace hasn't yet been initialized.
pub fn get_initial_kernel_namespace() -> Option<&'static Arc<CrateNamespace>> {
    INITIAL_KERNEL_NAMESPACE.get()
}

/// Returns the top-level directory that contains all of the namespaces. 
pub fn get_namespaces_directory() -> Option<DirRef> {
    root::get_root().lock().get_dir(NAMESPACES_DIRECTORY_NAME)
}


/// Create a new application `CrateNamespace` that uses the default application directory 
/// and is structured atop the given `recursive_namespace`. 
/// If no `recursive_namespace` is provided, the default initial kernel namespace will be used. 
/// 
/// # Return
/// The returned `CrateNamespace` will itself be empty, having no crates and no symbols in its map.
/// 
pub fn create_application_namespace(recursive_namespace: Option<Arc<CrateNamespace>>) -> Result<Arc<CrateNamespace>, &'static str> {
    // (1) use the initial kernel CrateNamespace as the new app namespace's recursive namespace if none was provided.
    let recursive_namespace = recursive_namespace
        .or_else(|| get_initial_kernel_namespace().cloned())
        .ok_or("initial kernel CrateNamespace not yet initialized")?;
    // (2) get the directory where the default app namespace should have been populated when mod_mgmt was inited.
    let default_app_namespace_name = CrateType::Application.default_namespace_name().to_string(); // this will be "_applications"
    let default_app_namespace_dir = get_namespaces_directory()
        .and_then(|ns_dir| ns_dir.lock().get_dir(&default_app_namespace_name))
        .ok_or("Couldn't find the directory for the default application CrateNamespace")?;
    // (3) create the actual new application CrateNamespace.
    let new_app_namespace = Arc::new(CrateNamespace::new(
        default_app_namespace_name,
        NamespaceDir::new(default_app_namespace_dir),
        Some(recursive_namespace),
    ));

    Ok(new_app_namespace)
}


/// Initializes the module management system based on the bootloader-provided modules, 
/// and creates and returns the default `CrateNamespace` for kernel crates.
pub fn init(
    bootloader_modules: Vec<BootloaderModule>,
    kernel_mmi: &mut MemoryManagementInfo
) -> Result<&'static Arc<CrateNamespace>, &'static str> {
    let (_namespaces_dir, default_kernel_namespace_dir) = parse_bootloader_modules_into_files(bootloader_modules, kernel_mmi)?;
    // Create the default CrateNamespace for kernel crates.
    let name = default_kernel_namespace_dir.lock().get_name();
    let default_namespace = CrateNamespace::new(name, default_kernel_namespace_dir, None);
    Ok(INITIAL_KERNEL_NAMESPACE.call_once(|| Arc::new(default_namespace)))
}


/// Parses the list of bootloader-loaded modules, turning them into crate object files, 
/// and placing them into namespace-specific directories according to their name prefix, e.g., "k#", "ksse#".
/// This function does not create any namespaces, it just populates the files and directories
/// such that namespaces can be created based on those files.
/// 
/// Returns a tuple of: 
/// * the top-level root "namespaces" directory that contains all other namespace directories,
/// * the directory of the default kernel crate namespace.
fn parse_bootloader_modules_into_files(
    bootloader_modules: Vec<BootloaderModule>,
    kernel_mmi: &mut MemoryManagementInfo
) -> Result<(DirRef, NamespaceDir), &'static str> {

    // create the top-level directory to hold all default namespaces
    let namespaces_dir = VFSDirectory::new(NAMESPACES_DIRECTORY_NAME.to_string(), root::get_root())?;

    // a map that associates a prefix string (e.g., "sse" in "ksse#crate.o") to a namespace directory of object files 
    let mut prefix_map: BTreeMap<String, NamespaceDir> = BTreeMap::new();

    // Closure to create the directory for a new namespace.
    let create_dir = |dir_name: &str| -> Result<NamespaceDir, &'static str> {
        VFSDirectory::new(dir_name.to_string(), &namespaces_dir).map(|d| NamespaceDir(d))
    };

    for m in bootloader_modules {
        let (crate_type, prefix, file_name) = CrateType::from_module_name(&m.name)?;
        let dir_name = format!("{}{}", prefix, crate_type.default_namespace_name());
        let name = String::from(file_name);

        let frames = allocate_frames_by_bytes_at(m.start, m.size_in_bytes())
            .map_err(|_e| "Failed to allocate frames for bootloader module")?;
        let pages = allocate_pages_by_bytes(m.size_in_bytes())
            .ok_or("Couldn't allocate virtual pages for bootloader module area")?;
        let mp = kernel_mmi.page_table.map_allocated_pages_to(
            pages, 
            frames, 
            EntryFlags::PRESENT, // we never need to write to bootloader-provided modules
        )?;

        // debug!("Module: {:?}, size {}, mp: {:?}", m.name, m.size_in_bytes(), mp);

        let create_file = |dir: &DirRef| {
            MemFile::from_mapped_pages(mp, name, m.size_in_bytes(), dir)
        };

        // Get the existing (or create a new) namespace directory corresponding to the given directory name.
        let _new_file = match prefix_map.entry(dir_name.clone()) {
            btree_map::Entry::Vacant(vacant) => create_file( vacant.insert(create_dir(&dir_name)?) )?,
            btree_map::Entry::Occupied(occ)  => create_file( occ.get() )?,
        };
    }

    debug!("Created namespace directories: {:?}", prefix_map.keys().map(|s| &**s).collect::<Vec<&str>>().join(", "));
    Ok((
        namespaces_dir,
        prefix_map.remove(CrateType::Kernel.default_namespace_name()).ok_or("BUG: no default namespace found")?,
    ))
}



/// A "symbol map" from a fully-qualified demangled symbol String  
/// to weak reference to a `LoadedSection`.
/// This is used for relocations, and for looking up function names.
pub type SymbolMap = Trie<BString, WeakSectionRef>;
pub type SymbolMapIter<'a> = qp_trie::Iter<'a, &'a BString, &'a WeakSectionRef>;


/// A wrapper around a `Directory` reference that offers special convenience functions
/// for getting and inserting crate object files into a directory.  
/// 
/// Auto-derefs into a `DirRef`.
#[derive(Clone)] 
pub struct NamespaceDir(DirRef);

impl Deref for NamespaceDir {
    type Target = DirRef;
    fn deref(&self) -> &DirRef {
        &self.0
    }
}

impl fmt::Debug for NamespaceDir {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if let Some(locked_dir) = self.0.try_lock() {
            write!(f, "{:?}", locked_dir.get_absolute_path())
        } else {
            write!(f, "<Locked>")
        }
    }
}

impl NamespaceDir {
    /// Creates a new `NamespaceDir` that wraps the given `DirRef`.
    pub fn new(dir: DirRef) -> NamespaceDir {
        NamespaceDir(dir)
    }    

    /// Finds the single file in this directory whose name starts with the given `prefix`.
    /// 
    /// # Return
    /// If a single file matches, then that file is returned. 
    /// Otherwise, if no files or multiple files match, then `None` is returned.
    pub fn get_file_starting_with(&self, prefix: &str) -> Option<FileRef> {
        let mut matching_files = self.get_files_starting_with(prefix).into_iter();
        matching_files.next()
            .filter(|_| matching_files.next().is_none()) // ensure single element
    }

    /// Returns the list of files in this Directory whose name starts with the given `prefix`.
    pub fn get_files_starting_with(&self, prefix: &str) -> Vec<FileRef> {
        let dir_locked = self.0.lock();
        let children = dir_locked.list();
        children.into_iter().filter_map(|name| {
            if name.starts_with(prefix) {
                dir_locked.get_file(&name)
            } else {
                None
            }
        }).collect()
    }

    /// Returns the list of file and directory names in this Directory whose name start with the given `prefix`.
    pub fn get_file_and_dir_names_starting_with(&self, prefix: &str) -> Vec<String> {
        let children = { self.0.lock().list() };
        children.into_iter()
            .filter(|name| name.starts_with(prefix))
            .collect()
    }

    /// Gets the given object file based on its crate name prefix. 
    /// 
    /// # Arguments
    /// * `crate_object_file_name`: the name of the object file to be returned, 
    ///    with or without a preceding `CrateType` prefix.
    /// 
    /// # Examples 
    /// * The name "k#keyboard-36be916209949cef.o" will look for and return the file "keyboard-36be916209949cef.o".
    /// * The name "keyboard-36be916209949cef.o" will look for and return the file "keyboard-36be916209949cef.o".
    /// * The name "a#ps.o" will look for and return the file "ps.o".
    pub fn get_crate_object_file(&self, crate_module_file_name: &str) -> Option<FileRef> {
        let (_crate_type, _prefix, objfilename) = CrateType::from_module_name(crate_module_file_name).ok()?;
        self.0.lock().get_file(objfilename)
    }

    /// Insert the given crate object file based on its crate type prefix. 
    /// 
    /// # Arguments
    /// * `crate_object_file_name`: the name of the object file to be inserted, 
    ///    with a preceding `CrateType` prefix.
    /// * `content`: the bytes that will be written into the file.
    /// 
    /// # Examples 
    /// * The file "k#keyboard-36be916209949cef.o" will be written to "./keyboard-36be916209949cef.o". 
    /// * The file "a#ps.o" will be placed into "./ps.o". 
    pub fn write_crate_object_file(&self, crate_object_file_name: &str, content: &[u8]) -> Result<FileRef, &'static str> {
        let (_crate_type, _prefix, objfilename) = CrateType::from_module_name(crate_object_file_name)?;
        let cfile = MemFile::new(String::from(objfilename), &self.0)?;
        cfile.lock().write(content, 0)?;
        Ok(cfile)
    }
}


/// A type that can be converted into a crate object file.
/// 
/// We use an enum rather than implement `TryInto` because we need additional information
/// to resolve a `Prefix`, namely the `CrateNamespace` in which to search for the prefix.
pub enum IntoCrateObjectFile {
    /// A direct reference to the crate object file. This will be used as-is. 
    File(FileRef),
    /// An absolute path that points to the crate object file. 
    AbsolutePath(Path),
    /// A string prefix that will be used to search for the crate object file in the namespace.
    /// This must be able to uniquely identify a single crate object file in the namespace directory (recursively searched). 
    Prefix(String),
}
impl fmt::Debug for IntoCrateObjectFile {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut dbg = f.debug_struct("IntoCrateObjectFile");
        match self {
            Self::File(object_file) => dbg.field("File", &object_file.try_lock()
                .map(|f| f.get_absolute_path())
                .unwrap_or_else(|| format!("<Locked>"))
            ),
            Self::AbsolutePath(p) => dbg.field("AbsolutePath", p),
            Self::Prefix(prefix) => dbg.field("Prefix", prefix),
        };
        dbg.finish()
    }
}


/// An application crate that has been loaded into a `CrateNamespace`.
/// 
/// This type auto-derefs into the application's `StrongCrateRef`.
/// 
/// When dropped, the application crate will be removed 
/// from the `CrateNamespace` into which it was originally loaded.
pub struct AppCrateRef {
    crate_ref: StrongCrateRef,
    namespace: Arc<CrateNamespace>,
}
impl Deref for AppCrateRef {
    type Target = StrongCrateRef;
    fn deref(&self) -> &StrongCrateRef {
        &self.crate_ref
    }
}
impl Drop for AppCrateRef {
    fn drop(&mut self) {
        // trace!("### Dropping AppCrateRef {:?} from namespace {:?}", self.crate_ref, self.namespace.name());
        let crate_locked = self.crate_ref.lock_as_ref();
        // First, remove the actual crate from the namespace.
        if let Some(_removed_app_crate) = self.namespace.crate_tree().lock().remove_str(&crate_locked.crate_name) {
            // Second, remove all of the crate's global symbols from the namespace's symbol map.
            let mut symbol_map = self.namespace.symbol_map().lock();
            for sec_to_remove in crate_locked.global_sections_iter() {
                if symbol_map.remove_str(&sec_to_remove.name).is_none() {
                    error!("NOTE: couldn't find old symbol {:?} in the old crate {:?} to remove from namespace {:?}.", sec_to_remove.name, crate_locked.crate_name, self.namespace.name());
                }
            }
        } else {
            error!("BUG: the dropped AppCrateRef {:?} could not be removed from namespace {:?}", self.crate_ref, self.namespace.name());
        }
    }
}


/// This struct represents a namespace of crates and their "global" (publicly-visible) symbols.
/// A crate namespace struct is basically a container around many crates 
/// that have all been loaded and linked against each other, 
/// completely separate and in isolation from any other crate namespace 
/// (although a given crate may be shared across multiple namespaces).
/// 
/// Each `CrateNamespace` can be treated as a separate OS personality, 
/// but are significantly more efficient than library OS-style personalities. 
/// A `CrateNamespace` is also useful to create a process (task group) abstraction.
/// 
/// `CrateNamespace`s can also optionally be recursive. 
/// For example, a namespace that holds just application crates and symbols 
/// can recursively rely upon (link against) the crates and symbols in a lower-level namespace
/// that contains kernel crates and symbols. 
pub struct CrateNamespace {
    /// An identifier for this namespace, just for convenience.
    name: String,

    /// The directory containing all crate object files owned by this namespace. 
    /// When this namespace is looking for a missing symbol or crate,
    /// it searches in this directory first.
    dir: NamespaceDir,

    /// The list of all the crates loaded into this namespace,
    /// stored as a map in which the crate's String name
    /// is the key that maps to the value, a strong reference to a crate.
    /// It is a strong reference because a crate must not be removed
    /// as long as it is part of any namespace,
    /// and a single crate can be part of multiple namespaces at once.
    /// For example, the "core" (Rust core library) crate is essentially
    /// part of every single namespace, simply because most other crates rely upon it. 
    crate_tree: Mutex<Trie<BString, StrongCrateRef>>,

    /// The "system map" of all symbols that are present in all of the crates in this `CrateNamespace`.
    /// Maps a fully-qualified symbol name string to a corresponding `LoadedSection`,
    /// which is guaranteed to be part of one of the crates in this `CrateNamespace`.  
    /// Symbols declared as "no_mangle" will appear in the map with no crate prefix, as expected.
    symbol_map: Mutex<SymbolMap>,

    /// The `CrateNamespace` that lies below this namespace, and can also be used by this namespace
    /// to resolve symbols and load crates that are relied on by other crates in this namespace.
    /// So, for example, if this namespace contains a set of application crates,
    /// its `recursive_namespace` could contain the set of kernel crates that these application crates rely on.
    recursive_namespace: Option<Arc<CrateNamespace>>,

    /// A setting that toggles whether to ignore hash differences in symbols when resolving a dependency. 
    /// For example, if `true`, the symbol `my_crate::foo::h123` will be used to satisfy a dependency 
    /// on any other `my_crate::foo::*` regardless of hash value. 
    /// Fuzzy matching should only be successful if there is just a single matching symbol; 
    /// if there are multiple matches (e.g., `my_crate::foo::h456` and `my_crate::foo::h789` both exist),
    /// then the dependency should fail to be resolved.
    /// 
    /// This is a potentially dangerous setting because it overrides the compiler-chosen dependency links.
    /// Thus, it is false by default, and should only be enabled with expert knowledge, 
    /// ideally only temporarily in order to manually load a given crate.
    fuzzy_symbol_matching: bool,
}

impl CrateNamespace {
    /// Creates a new `CrateNamespace` that is completely empty (no loaded crates).
    /// # Arguments
    /// * `name`: the name of this `CrateNamespace`, used only for convenience purposes.
    /// * `dir`: the directory of crate object files for this namespace.
    /// * `recursive_namespace`: another `CrateNamespace` that can optionally be used 
    ///    to recursively resolve missing crates/symbols. 
    pub fn new(name: String, dir: NamespaceDir, recursive_namespace: Option<Arc<CrateNamespace>>) -> CrateNamespace {
        CrateNamespace {
            name,
            dir,
            recursive_namespace,
            crate_tree: Mutex::new(Trie::new()),
            symbol_map: Mutex::new(SymbolMap::new()),
            fuzzy_symbol_matching: false,
        }
    } 

    /// Returns the name of this `CrateNamespace`, which is just used for debugging purposes. 
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the directory that this `CrateNamespace` is based on.
    pub fn dir(&self) -> &NamespaceDir {
        &self.dir
    }

    /// Returns the recursive namespace that this `CrateNamespace` is built atop,
    /// if one exists.
    pub fn recursive_namespace(&self) -> Option<&Arc<CrateNamespace>> {
        self.recursive_namespace.as_ref()
    }

    #[doc(hidden)]
    pub fn crate_tree(&self) -> &Mutex<Trie<BString, StrongCrateRef>> {
        &self.crate_tree
    }

    #[doc(hidden)]
    pub fn symbol_map(&self) -> &Mutex<SymbolMap> {
        &self.symbol_map
    }

    #[doc(hidden)]
    pub fn enable_fuzzy_symbol_matching(&mut self) {
        self.fuzzy_symbol_matching = true;
    }

    #[doc(hidden)]
    pub fn disable_fuzzy_symbol_matching(&mut self) {
        self.fuzzy_symbol_matching = false;
    }

    /// Returns a list of all of the crate names currently loaded into this `CrateNamespace`,
    /// including all crates in any recursive namespaces as well if `recursive` is `true`.
    /// This is a slow method mostly for debugging, since it allocates new Strings for each crate name.
    pub fn crate_names(&self, recursive: bool) -> Vec<String> {
        let mut crates: Vec<String> = self.crate_tree.lock().keys().map(|bstring| String::from(bstring.as_str())).collect();

        if recursive {
            if let Some(mut crates_recursive) = self.recursive_namespace.as_ref().map(|r_ns| r_ns.crate_names(recursive)) {
                crates.append(&mut crates_recursive);
            }
        }
        crates
    }

    /// Iterates over all crates in this namespace and calls the given function `f` on each crate.
    /// If `recursive` is true, crates in recursive namespaces are included in the iteration as well.
    /// 
    /// The function `f` is called with two arguments: the name of the crate, and a reference to the crate.
    /// The function `f` must return a boolean value that indicates whether to continue iterating; 
    /// if `true`, the iteration will continue, if `false`, the iteration will stop. 
    pub fn for_each_crate<F>(
        &self,
        recursive: bool,
        mut f: F
    ) where F: FnMut(&str, &StrongCrateRef) -> bool {
        for (crate_name, crate_ref) in self.crate_tree.lock().iter() {
            let keep_going = f(crate_name.as_str(), crate_ref);
            if !keep_going {
                return;
            }
        }

        if recursive {
            if let Some(ref r_ns) = self.recursive_namespace {
                r_ns.for_each_crate(recursive, f);
            }
        }
    }

    /// Acquires the lock on this `CrateNamespace`'s crate list and returns the crate 
    /// that matches the given `crate_name`, if it exists in this namespace.
    /// If it does not exist in this namespace, then the recursive namespace is searched as well.
    /// 
    /// # Important note about Return value
    /// Returns a `StrongCrateReference` that **has not** been marked as a shared crate reference,
    /// so if the caller wants to keep the returned `StrongCrateRef` as a shared crate 
    /// that jointly exists in another namespace, they should invoke the 
    /// [`CowArc::share()`](cow_arc/CowArc.share.html) function on the returned value.
    pub fn get_crate(&self, crate_name: &str) -> Option<StrongCrateRef> {
        self.crate_tree.lock().get_str(crate_name)
            .map(|c| CowArc::clone_shallow(c))
            .or_else(|| self.recursive_namespace.as_ref().and_then(|r_ns| r_ns.get_crate(crate_name)))
    }

    /// Acquires the lock on this `CrateNamespace`'s crate list and returns the crate 
    /// that matches the given `crate_name`, if it exists in this namespace.
    /// If it does not exist in this namespace, then the recursive namespace is searched as well.
    ///
    /// This function is similar to the [`get_crate`](#method.get_crate) method,
    /// but it also returns the `CrateNamespace` in which the crate was found.
    /// It is an associated function rather than a method so it can operate on `Arc<CrateNamespace>`s.
    /// 
    /// # Important note about Return value
    /// Returns a `StrongCrateReference` that **has not** been marked as a shared crate reference,
    /// so if the caller wants to keep the returned `StrongCrateRef` as a shared crate 
    /// that jointly exists in another namespace, they should invoke the 
    /// [`CowArc::share()`](cow_arc/CowArc.share.html) function on the returned value.
    pub fn get_crate_and_namespace<'n>(
        namespace: &'n Arc<CrateNamespace>,
        crate_name: &str
    ) -> Option<(StrongCrateRef, &'n Arc<CrateNamespace>)> {
        namespace.crate_tree.lock().get_str(crate_name)
            .map(|c| (CowArc::clone_shallow(c), namespace))
            .or_else(|| namespace.recursive_namespace.as_ref().and_then(|r_ns| Self::get_crate_and_namespace(r_ns, crate_name)))
    }

    /// Finds the `LoadedCrate`s whose names start with the given `crate_name_prefix`.
    /// 
    /// # Return
    /// Returns a list of matching crates, in the form of a tuple containing the crate's name, 
    /// a shallow-cloned reference to the crate, and a reference to the namespace in which the matching crate was found.
    /// If you want to add the returned crate to another namespace, 
    /// you MUST fully `clone()` the returned crate reference in order to mark that crate as shared across namespaces. 
    /// 
    /// # Important Usage Note
    /// To avoid greedily matching more crates than expected, you may wish to end the `crate_name_prefix` with "`-`".
    /// This may provide results more in line with the caller's expectations; see the last example below about a trailing "`-`". 
    /// This works because the delimiter between a crate name and its trailing hash value is "`-`".
    /// 
    /// # Example
    /// * This `CrateNamespace` contains the crates `my_crate-843a613894da0c24` and 
    ///   `my_crate_new-933a635894ce0f12`. 
    ///   Calling `get_crates_starting_with("my_crate")` will return both crates,
    pub fn get_crates_starting_with<'n>(
        namespace: &'n Arc<CrateNamespace>,
        crate_name_prefix: &str
    ) -> Vec<(String, StrongCrateRef, &'n Arc<CrateNamespace>)> { 
        // First, we make a list of matching crates in this namespace. 
        let crates = namespace.crate_tree.lock();
        let mut crates_in_this_namespace = crates.iter_prefix_str(crate_name_prefix)
            .map(|(key, val)| (key.clone().into(), val.clone_shallow(), namespace))
            .collect::<Vec<_>>();

        // Second, we make a similar list for the recursive namespace.
        let mut crates_in_recursive_namespace = namespace.recursive_namespace.as_ref()
            .map(|r_ns| Self::get_crates_starting_with(r_ns, crate_name_prefix))
            .unwrap_or_default();

        // Third, we combine the lists into one list that spans all namespaces.
        crates_in_this_namespace.append(&mut crates_in_recursive_namespace);
        crates_in_this_namespace        
    }

    /// Finds the `LoadedCrate` whose name starts with the given `crate_name_prefix`,
    /// *if and only if* there is a single matching crate in this namespace or any of its recursive namespaces.
    /// This is a convenience wrapper around the [`get_crates_starting_with()`](#method.get_crates_starting_with) method. 
    /// 
    /// # Return
    /// Returns a tuple containing the crate's name, a shallow-cloned reference to the crate, 
    /// and a reference to the namespace in which the matching crate was found.
    /// If you want to add the returned crate to another namespace, 
    /// you MUST fully `clone()` the returned crate reference in order to mark that crate as shared across namespaces. 
    /// 
    /// # Important Usage Note
    /// To avoid greedily matching more crates than expected, you may wish to end the `crate_name_prefix` with "`-`".
    /// This may provide results more in line with the caller's expectations; see the last example below about a trailing "`-`". 
    /// This works because the delimiter between a crate name and its trailing hash value is "`-`".
    /// 
    /// # Example
    /// * This `CrateNamespace` contains the crates `my_crate-843a613894da0c24` and 
    ///   `my_crate_new-933a635894ce0f12`. 
    ///   Calling `get_crate_starting_with("my_crate")` will return None,
    ///   because it will match both `my_crate` and `my_crate_new`. 
    ///   To match only `my_crate`, call this function as `get_crate_starting_with("my_crate-")`.
    pub fn get_crate_starting_with<'n>(
        namespace: &'n Arc<CrateNamespace>,
        crate_name_prefix: &str
    ) -> Option<(String, StrongCrateRef, &'n Arc<CrateNamespace>)> { 
        let mut crates_iter = Self::get_crates_starting_with(namespace, crate_name_prefix).into_iter();
        crates_iter.next().filter(|_| crates_iter.next().is_none()) // ensure single element
    }

    /// Like [`get_crates_starting_with()`](#method.get_crates_starting_with),
    /// but for crate *object file*s instead of loaded crates. 
    /// 
    /// Returns a list of matching object files and the namespace in which they were found,
    /// inclusive of recursive namespaces.
    pub fn get_crate_object_files_starting_with<'n>(
        namespace: &'n Arc<CrateNamespace>,
        file_name_prefix: &str
    ) -> Vec<(FileRef, &'n Arc<CrateNamespace>)> { 
        // First, we make a list of matching files in this namespace. 
        let mut files = namespace.dir
            .get_files_starting_with(file_name_prefix)
            .into_iter()
            .map(|f| (f, namespace))
            .collect::<Vec<_>>();

        // Second, we make a similar list for the recursive namespace.
        let mut files_in_recursive_namespace = namespace.recursive_namespace.as_ref()
            .map(|r_ns| Self::get_crate_object_files_starting_with(r_ns, file_name_prefix))
            .unwrap_or_default();

        // Third, we combine the lists into one list that spans all namespaces.
        files.append(&mut files_in_recursive_namespace);
        files        
    }

    /// Like [`get_crate_starting_with()`](#method.get_crate_starting_with),
    /// but for crate *object file*s instead of loaded crates. 
    /// 
    /// Returns the matching object file and the namespace in which it was found,
    /// if and only if there was a single match (inclusive of recursive namespaces).
    pub fn get_crate_object_file_starting_with<'n>(
        namespace: &'n Arc<CrateNamespace>,
        file_name_prefix: &str
    ) -> Option<(FileRef, &'n Arc<CrateNamespace>)> { 
        let mut files_iter = Self::get_crate_object_files_starting_with(namespace, file_name_prefix).into_iter();
        files_iter.next().filter(|_| files_iter.next().is_none()) // ensure single element
    }


    /// Same as `get_crate_object_files_starting_with()`,
    /// but is a method instead of an associated function,
    /// and also returns `&CrateNamespace` instead of `&Arc<CrateNamespace>`.
    /// 
    /// This is only necessary because I can't figure out how to make a generic function
    /// that accepts and returns either `&CrateNamespace` or `&Arc<CrateNamespace>`.
    fn method_get_crate_object_files_starting_with(
        &self,
        file_name_prefix: &str
    ) -> Vec<(FileRef, &CrateNamespace)> { 
        // First, we make a list of matching files in this namespace. 
        let mut files = self.dir
            .get_files_starting_with(file_name_prefix)
            .into_iter()
            .map(|f| (f, self))
            .collect::<Vec<_>>();

        // Second, we make a similar list for the recursive namespace.
        let mut files_in_recursive_namespace = self.recursive_namespace.as_ref()
            .map(|r_ns| r_ns.method_get_crate_object_files_starting_with(file_name_prefix))
            .unwrap_or_default();

        // Third, we combine the lists into one list that spans all namespaces.
        files.append(&mut files_in_recursive_namespace);
        files        
    }

    /// Same as `get_crate_object_file_starting_with()`,
    /// but is a method instead of an associated function,
    /// and also returns `&CrateNamespace` instead of `&Arc<CrateNamespace>`.
    /// 
    /// This is only necessary because I can't figure out how to make a generic function
    /// that accepts and returns either `&CrateNamespace` or `&Arc<CrateNamespace>`.
    fn method_get_crate_object_file_starting_with(
        &self,
        file_name_prefix: &str
    ) -> Option<(FileRef, &CrateNamespace)> { 
        let mut files_iter = self.method_get_crate_object_files_starting_with(file_name_prefix).into_iter();
        files_iter.next().filter(|_| files_iter.next().is_none()) // ensure single element
    }

    /// Loads the specified application crate into this `CrateNamespace`, allowing it to be run.
    /// 
    /// The new application crate's public symbols are added to this `CrateNamespace`'s symbol map,
    /// allowing other crates in this namespace to depend upon it.
    /// 
    /// Application crates are added to the CrateNamespace just like kernel crates,
    /// so to load an application crate multiple times to spawn multiple instances of it,
    /// you can create a new top-level namespace to hold that application crate.
    /// 
    /// Returns a Result containing the newly-loaded application crate itself.
    pub fn load_crate_as_application(
        namespace: &Arc<CrateNamespace>,
        crate_object_file: &FileRef, 
        kernel_mmi_ref: &MmiRef, 
        verbose_log: bool
    ) -> Result<AppCrateRef, &'static str> {
        debug!("load_crate_as_application(): trying to load application crate at {:?}", crate_object_file.lock().get_absolute_path());
        // Don't use a backup namespace when loading applications;
        // we must be able to find all symbols in only this namespace and its backing recursive namespaces.
        let new_crate_ref = namespace.load_crate_internal(crate_object_file, None, kernel_mmi_ref, verbose_log)?;
        {
            let new_crate = new_crate_ref.lock_as_ref();
            let _new_syms = namespace.add_symbols(new_crate.sections.values(), verbose_log);
            namespace.crate_tree.lock().insert_str(&new_crate.crate_name, CowArc::clone_shallow(&new_crate_ref));
            info!("loaded new application crate: {:?}, num sections: {}, added {} new symbols", new_crate.crate_name, new_crate.sections.len(), _new_syms);
        }
        Ok(AppCrateRef {
            crate_ref: new_crate_ref,
            namespace: Arc::clone(namespace),
        })
    }


    /// Loads the specified crate into memory, allowing it to be invoked.  
    /// Returns a Result containing the number of symbols that were added to the symbol map
    /// as a result of loading this crate.
    /// 
    /// # Arguments
    /// * `crate_object_file`: the crate object file that will be loaded into this `CrateNamespace`.
    /// * `temp_backup_namespace`: the `CrateNamespace` that should be searched for missing symbols 
    ///   (for relocations) if a symbol cannot be found in this `CrateNamespace`. 
    ///   If `temp_backup_namespace` is `None`, then no other namespace will be searched, 
    ///   and any missing symbols will return an `Err`. 
    /// * `kernel_mmi_ref`: a mutable reference to the kernel's `MemoryManagementInfo`.
    /// * `verbose_log`: a boolean value whether to enable verbose_log logging of crate loading actions.
    pub fn load_crate(
        &self,
        crate_object_file: &FileRef,
        temp_backup_namespace: Option<&CrateNamespace>, 
        kernel_mmi_ref: &MmiRef, 
        verbose_log: bool
    ) -> Result<(StrongCrateRef, usize), &'static str> {

        #[cfg(not(loscd_eval))]
        debug!("load_crate: trying to load crate at {:?}", crate_object_file.lock().get_absolute_path());
        let new_crate_ref = self.load_crate_internal(crate_object_file, temp_backup_namespace, kernel_mmi_ref, verbose_log)?;
        
        let (new_crate_name, _num_sections, new_syms) = {
            let new_crate = new_crate_ref.lock_as_ref();
            let new_syms = self.add_symbols(new_crate.sections.values(), verbose_log);
            (new_crate.crate_name.clone(), new_crate.sections.len(), new_syms)
        };
            
        #[cfg(not(loscd_eval))]
        info!("loaded new crate {:?}, num sections: {}, added {} new symbols.", new_crate_name, _num_sections, new_syms);
        self.crate_tree.lock().insert(new_crate_name.into(), new_crate_ref.clone_shallow());
        Ok((new_crate_ref, new_syms))
    }


    /// The internal function that does the work for loading crates,
    /// but does not add the crate nor its symbols to this namespace. 
    /// See [`load_crate`](#method.load_crate) and [`load_crate_as_application`](#fn.load_crate_as_application).
    fn load_crate_internal(&self,
        crate_object_file: &FileRef,
        temp_backup_namespace: Option<&CrateNamespace>, 
        kernel_mmi_ref: &MmiRef, 
        verbose_log: bool
    ) -> Result<StrongCrateRef, &'static str> {
        let cf = crate_object_file.lock();
        let (new_crate_ref, elf_file) = self.load_crate_sections(cf.deref(), kernel_mmi_ref, verbose_log)?;
        self.perform_relocations(&elf_file, &new_crate_ref, temp_backup_namespace, kernel_mmi_ref, verbose_log)?;
        Ok(new_crate_ref)
    }

    
    /// This function first loads all of the given crates' sections and adds them to the symbol map,
    /// and only after *all* crates are loaded does it move on to linking/relocation calculations. 
    /// 
    /// This allows multiple object files with circular dependencies on one another
    /// to be loaded all at once, as if they were a single entity.
    /// 
    /// # Example
    /// If crate `A` depends on crate `B`, and crate `B` depends on crate `A`,
    /// this function will load both crate `A` and `B` before trying to resolve their dependencies individually. 
    pub fn load_crates<'f, I>(
        &self,
        crate_files: I,
        temp_backup_namespace: Option<&CrateNamespace>,
        kernel_mmi_ref: &MmiRef,
        verbose_log: bool,
    ) -> Result<(), &'static str> 
        where I: Iterator<Item = &'f FileRef>
    {
        // First, lock all of the crate object files.
        let mut locked_crate_files = Vec::new();
        for crate_file_ref in crate_files {
            locked_crate_files.push(crate_file_ref.lock());
        }

        // Second, do all of the section parsing and loading, and add all public symbols to the symbol map.
        let mut partially_loaded_crates: Vec<(StrongCrateRef, ElfFile)> = Vec::with_capacity(locked_crate_files.len()); 
        for locked_crate_file in &locked_crate_files {            
            let (new_crate_ref, elf_file) = self.load_crate_sections(locked_crate_file.deref(), kernel_mmi_ref, verbose_log)?;
            let _new_syms = self.add_symbols(new_crate_ref.lock_as_ref().sections.values(), verbose_log);
            partially_loaded_crates.push((new_crate_ref, elf_file));
        }
        
        // Finally, we do all of the relocations.
        for (new_crate_ref, elf_file) in partially_loaded_crates {
            self.perform_relocations(&elf_file, &new_crate_ref, temp_backup_namespace, kernel_mmi_ref, verbose_log)?;
            let name = new_crate_ref.lock_as_ref().crate_name.clone();
            self.crate_tree.lock().insert(name.into(), new_crate_ref);
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
    /// When modifying crates in the new namespace, e.g., swapping crates, 
    /// any crates in the new namespace that are still shared with the old namespace
    /// must be deeply copied into a new crate that is exclusively owned,
    /// and then that new crate will be modified in whichever way desired. 
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
        CrateNamespace {
            name: self.name.clone(),
            dir: self.dir.clone(),
            recursive_namespace: self.recursive_namespace.clone(),
            crate_tree: Mutex::new(self.crate_tree.lock().clone()),
            symbol_map: Mutex::new(self.symbol_map.lock().clone()),
            fuzzy_symbol_matching: self.fuzzy_symbol_matching,
        }
    }


    /// Finds all of the weak dependents (sections that depend on the given `old_section`)
    /// and rewrites their relocation entries to point to the given `new_section`.
    /// This effectively replaces the usage of the `old_section` with the `new_section`,
    /// but does not make any modifications to symbol maps.
    pub fn rewrite_section_dependents(
        old_section: &StrongSectionRef,
        new_section: &StrongSectionRef,
        kernel_mmi_ref: &MmiRef
    ) -> Result<(), &'static str> {

        for weak_dep in &old_section.inner.read().sections_dependent_on_me {
            let target_sec = weak_dep.section.upgrade().ok_or_else(|| "couldn't upgrade WeakDependent.section")?;
            let relocation_entry = weak_dep.relocation;

            debug!("rewrite_section_dependents(): target_sec: {:?}, old_sec: {:?}, new_sec: {:?}", target_sec, old_section, new_section);

            // If the target_sec's mapped pages aren't writable (which is common in the case of swapping),
            // then we need to temporarily remap them as writable here so we can fix up the target_sec's new relocation entry.
            {
                let mut target_sec_mapped_pages = target_sec.mapped_pages.lock();
                let target_sec_initial_flags = target_sec_mapped_pages.flags();
                if !target_sec_initial_flags.is_writable() {
                    target_sec_mapped_pages.remap(&mut kernel_mmi_ref.lock().page_table, target_sec_initial_flags | EntryFlags::WRITABLE)?;
                }

                write_relocation(
                    relocation_entry, 
                    &mut target_sec_mapped_pages, 
                    target_sec.mapped_pages_offset, 
                    new_section.start_address(), 
                    false
                )?;

                // If we temporarily remapped the target_sec's mapped pages as writable, undo that here
                if !target_sec_initial_flags.is_writable() {
                    target_sec_mapped_pages.remap(&mut kernel_mmi_ref.lock().page_table, target_sec_initial_flags)?;
                };
            }
            
            // Tell the new source_sec that the existing target_sec depends on it.
            // Note that we don't need to do this if we're re-swapping in a cached crate,
            // because that crate's sections' dependents are already properly set up from when it was first swapped in.
            // if !is_optimized {
                new_section.inner.write().sections_dependent_on_me.push(WeakDependent {
                    section: Arc::downgrade(&target_sec),
                    relocation: relocation_entry,
                });
            // }

            // Tell the existing target_sec that it no longer depends on the old source section (old_sec),
            // and that it now depends on the new source_sec.
            let mut found_strong_dependency = false;
            for mut strong_dep in target_sec.inner.write().sections_i_depend_on.iter_mut() {
                if Arc::ptr_eq(&strong_dep.section, old_section) && strong_dep.relocation == relocation_entry {
                    strong_dep.section = Arc::clone(new_section);
                    found_strong_dependency = true;
                    break;
                }
            }
            if !found_strong_dependency {
                error!("Couldn't find/remove the existing StrongDependency from target_sec {:?} to old_sec {:?}",
                    target_sec.name, old_section.name);
                return Err("Couldn't find/remove the target_sec's StrongDependency on the old crate section");
            }
        }

        Ok(())
    }


    /// The primary internal routine for parsing and loading all sections in a crate object file.
    /// This does not perform any relocations or linking, so the crate **is not yet ready to use after this function**,
    /// since its sections are totally incomplete and non-executable.
    /// 
    /// However, it does add all of the newly-loaded crate sections to the symbol map (yes, even before relocation/linking),
    /// since we can use them to resolve missing symbols for relocations.
    /// 
    /// Parses each section in the given `crate_file` object file and copies its contents to each section.
    /// Returns a tuple of a reference to the new `LoadedCrate` and the crate's ELF file (to avoid having to re-parse it).
    /// 
    /// # Arguments
    /// * `crate_file`: the object file for the crate that will be loaded into this `CrateNamespace`.
    /// * `kernel_mmi_ref`: the kernel's MMI struct, for memory mapping use.
    /// * `verbose_log`: whether to log detailed messages for debugging.
    fn load_crate_sections<'f>(
        &self,
        crate_file: &'f dyn File,
        kernel_mmi_ref: &MmiRef,
        _verbose_log: bool
    ) -> Result<(StrongCrateRef, ElfFile<'f>), &'static str> {
        
        let mapped_pages  = crate_file.as_mapping()?;
        let size_in_bytes = crate_file.size();
        let abs_path      = Path::new(crate_file.get_absolute_path());
        let crate_name    = crate_name_from_path(&abs_path).to_string();

        // First, check to make sure this crate hasn't already been loaded. 
        // Application crates are now added to the CrateNamespace just like kernel crates,
        // so to load an application crate multiple times and run multiple instances of it,
        // you can create a top-level new namespace to hold that application crate.
        if self.get_crate(&crate_name).is_some() {
            return Err("the crate has already been loaded, cannot load it again in the same namespace");
        }

        // It's probably better to pass in the actual crate file reference so we can use it here,
        // but since we don't currently do that, we just get another reference to the crate object file via its Path.
        let crate_object_file = match Path::get_absolute(&abs_path) {
            Some(FileOrDir::File(f)) => f, 
            _ => return Err("BUG: load_crate_sections(): couldn't get crate object file path"),
        };

        // Parse the crate file as an ELF file
        let byte_slice: &[u8] = mapped_pages.as_slice(0, size_in_bytes)?;
        let elf_file = ElfFile::new(byte_slice)?; // returns Err(&str) if ELF parse fails

        // check that elf_file is a relocatable type 
        use xmas_elf::header::Type;
        let typ = elf_file.header.pt2.type_().as_type();
        if typ != Type::Relocatable {
            error!("load_crate_sections(): crate \"{}\" was a {:?} Elf File, must be Relocatable!", &crate_name, typ);
            return Err("not a relocatable elf file");
        }

        #[cfg(not(loscd_eval))]
        debug!("Parsing Elf kernel crate: {:?}, size {:#x}({})", abs_path, size_in_bytes, size_in_bytes);

        // allocate enough space to load the sections
        let section_pages = allocate_section_pages(&elf_file, kernel_mmi_ref)?;
        let text_pages   = section_pages.executable_pages.map(|(tp, range)| (Arc::new(Mutex::new(tp)), range));
        let rodata_pages = section_pages.read_only_pages.map( |(rp, range)| (Arc::new(Mutex::new(rp)), range));
        let data_pages   = section_pages.read_write_pages.map(|(dp, range)| (Arc::new(Mutex::new(dp)), range));

        // Check the symbol table to get the set of sections that are global (publicly visible).
        let global_sections: BTreeSet<Shndx> = {
            // For us to properly load the ELF file, it must NOT have been fully stripped,
            // meaning that it must still have its symbol table section. Otherwise, relocations will not work.
            let symtab = find_symbol_table(&elf_file)?;

            let mut globals: BTreeSet<Shndx> = BTreeSet::new();
            use xmas_elf::symbol_table::Entry;
            for entry in symtab.iter() {
                // Include all symbols with "GLOBAL" binding, regardless of visibility.  
                if entry.get_binding() == Ok(xmas_elf::symbol_table::Binding::Global) {
                    if let Ok(typ) = entry.get_type() {
                        if typ == xmas_elf::symbol_table::Type::Func || typ == xmas_elf::symbol_table::Type::Object {
                            globals.insert(entry.shndx() as Shndx);
                        }
                    }
                }
            }
            globals 
        };

        // Since .text sections come at the beginning of the object file,
        // we can simply directly copy all .text sections at once,
        // ranging from the beginning of the file to the end of the last .text section.
        // We actually must do it this way, though it has the following tradeoffs:
        // (+) It's more correct than calculating the minimum required size of each individual .text section,
        //     because other sections (e.g., eh_frame) rely on the layout of loaded .text sections 
        //     to be identical to their layout (offsets) specified in the object file.
        //     Otherwise, parsing debug/frame sections won't work properly.
        // (+) It's way faster to load the sections, since we can just bulk copy all .text sections at once 
        //     instead of copying them individually on a per-section basis (or just remap their pages directly).
        // (-) It ends up wasting a few hundred bytes here and there, but almost always under 100 bytes.
        if let Some((ref tp, ref tp_range)) = text_pages {
            let text_size = tp_range.end.value() - tp_range.start.value();
            let mut tp_locked = tp.lock();
            let text_destination: &mut [u8] = tp_locked.as_slice_mut(0, text_size)?;
            let text_source = elf_file.input.get(..text_size).ok_or("BUG: end of last .text section was miscalculated to be beyond ELF file bounds")?;
            text_destination.copy_from_slice(text_source);
        }

        // Because .rodata, .data, and .bss may be intermingled, 
        // we copy them into their respective pages individually on a per-section basis, 
        // keeping track of the offset into each of their MappedPages as we go.
        let (mut rodata_offset, mut data_offset) = (0 , 0);
                    
        const TEXT_PREFIX:           &'static str = ".text.";
        const RODATA_PREFIX:         &'static str = ".rodata.";
        const DATA_PREFIX:           &'static str = ".data.";
        const BSS_PREFIX:            &'static str = ".bss.";
        const RELRO_PREFIX:          &'static str = "rel.ro.";
        const GCC_EXCEPT_TABLE_NAME: &'static str = ".gcc_except_table";
        const EH_FRAME_NAME:         &'static str = ".eh_frame";

        let new_crate = CowArc::new(LoadedCrate {
            crate_name:              crate_name.clone(),
            debug_symbols_file:      Arc::downgrade(&crate_object_file),
            object_file:             crate_object_file, 
            sections:                HashMap::new(),
            text_pages:              text_pages.clone(),
            rodata_pages:            rodata_pages.clone(),
            data_pages:              data_pages.clone(),
            global_sections:         BTreeSet::new(),
            data_sections:           BTreeSet::new(),
            reexported_symbols:      BTreeSet::new(),
        });
        let new_crate_weak_ref = CowArc::downgrade(&new_crate);
        
        // this maps section header index (shndx) to LoadedSection
        let mut loaded_sections: HashMap<Shndx, StrongSectionRef> = HashMap::new(); 
        // the set of Shndxes for .data and .bss sections
        let mut data_sections: BTreeSet<Shndx> = BTreeSet::new();

        let mut read_only_pages_locked  = rodata_pages.as_ref().map(|(rp, _)| (rp.clone(), rp.lock()));
        let mut read_write_pages_locked = data_pages  .as_ref().map(|(dp, _)| (dp.clone(), dp.lock()));


        // In this loop, we handle only "allocated" sections that occupy memory in the actual loaded object file.
        // This includes .text, .rodata, .data, .bss, .gcc_except_table, .eh_frame, and potentially others.
        for (shndx, sec) in elf_file.section_iter().enumerate() {
            let sec_flags = sec.flags();
            // Skip non-allocated sections, because they don't appear in the loaded object file.
            if sec_flags & SHF_ALLOC == 0 {
                continue; 
            }

            // Even if we're using the next section's data (for a zero-sized section, as handled below),
            // we still want to use this current section's actual name and flags!
            let sec_name = match sec.get_name(&elf_file) {
                Ok(name) => name,
                Err(_e) => {
                    error!("load_crate_sections: couldn't get section name for section [{}]: {:?}\n    error: {}", shndx, sec, _e);
                    return Err("couldn't get section name");
                }
            };

            // ignore the empty .text section at the start
            if sec_name == ".text" {
                continue;    
            }

            // This handles the rare case of a zero-sized section. 
            // A section of size zero shouldn't necessarily be removed, as they are sometimes referenced in relocations;
            // typically the zero-sized section itself is a reference to the next section in the list of section headers.
            // Thus, we need to use the *current* section's name with the *next* section's information,
            // i.e., its  size, alignment, and actual data.
            let sec = if sec.size() == 0 {
                match elf_file.section_header((shndx + 1) as u16) { // get the next section
                    Ok(next_sec) => {
                        // The next section must have the same offset as the current zero-sized one
                        if next_sec.offset() == sec.offset() {
                            // if it does, we can use it in place of the current section
                            next_sec
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

            let write: bool = sec_flags & SHF_WRITE     == SHF_WRITE;
            let exec:  bool = sec_flags & SHF_EXECINSTR == SHF_EXECINSTR;

            // First, check for executable sections, which can only be .text sections.
            if exec && !write {
                if let Some(name) = sec_name.get(TEXT_PREFIX.len() ..) {
                    let demangled = demangle(name).to_string();

                    // We already copied the content of all .text sections above, 
                    // so here we just record the metadata into a new `LoadedSection` object.
                    if let Some((ref tp_ref, ref tp_range)) = text_pages {
                        let text_offset = sec.offset() as usize;
                        let dest_vaddr = tp_range.start + text_offset;

                        loaded_sections.insert(
                            shndx, 
                            Arc::new(LoadedSection::new(
                                SectionType::Text,
                                demangled,
                                Arc::clone(tp_ref),
                                text_offset,
                                dest_vaddr,
                                sec_size,
                                global_sections.contains(&shndx),
                                new_crate_weak_ref.clone(),
                            ))
                        );
                    }
                    else {
                        return Err("BUG: ELF file contained a .text* section, but no text_pages were allocated");
                    }
                }
                else {
                    error!("Failed to get the .text section's name after \".text.\": {:?}", sec_name);
                    return Err("Failed to get the .text section's name after \".text.\"!");
                }
            }

            // Second, if not executable, handle writable .data/.bss sections
            else if write {
                // check if this section is .bss or .data
                let (name, is_bss) = if sec_name.starts_with(BSS_PREFIX) {
                    if let Some(name) = sec_name.get(BSS_PREFIX.len() ..) {
                        (name, true) // true means it is .bss
                    } else {
                        error!("Failed to get the .bss section's name after \".bss.\": {:?}", sec_name);
                        return Err("Failed to get the .bss section's name after \".bss.\"!");
                    }
                } else if sec_name.starts_with(DATA_PREFIX) {
                    if let Some(name) = sec_name.get(DATA_PREFIX.len() ..) {
                        let name = if name.starts_with(RELRO_PREFIX) {
                            let relro_name = name.get(RELRO_PREFIX.len() ..).ok_or("Couldn't get name of .data.rel.ro. section")?;
                            relro_name
                        } else {
                            name
                        };
                        (name, false) // false means it's not .bss
                    }
                    else {
                        error!("Failed to get the .data section's name after \".data.\": {:?}", sec_name);
                        return Err("Failed to get the .data section's name after \".data.\"!");
                    }
                } else {
                    error!("Unsupported: found writable section that wasn't .data or .bss: [{}] {:?}", shndx, sec_name);
                    return Err("Unsupported: found writable section that wasn't .data or .bss");
                };
                let demangled = demangle(name).to_string();
                
                if let Some((ref dp_ref, ref mut dp)) = read_write_pages_locked {
                    // here: we're ready to copy the data/bss section to the proper address
                    let dest_vaddr = dp.address_at_offset(data_offset)
                        .ok_or_else(|| "BUG: data_offset wasn't within data_pages")?;
                    let dest_slice: &mut [u8] = dp.as_slice_mut(data_offset, sec_size)?;
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
                    
                    loaded_sections.insert(
                        shndx,
                        Arc::new(LoadedSection::new(
                            if is_bss { SectionType::Bss } else { SectionType::Data },
                            demangled.clone(),
                            Arc::clone(dp_ref),
                            data_offset,
                            dest_vaddr,
                            sec_size,
                            global_sections.contains(&shndx),
                            new_crate_weak_ref.clone(),
                        ))
                    );
                    data_sections.insert(shndx);

                    data_offset += round_up_power_of_two(sec_size, sec_align);
                }
                else {
                    return Err("no data_pages were allocated for .data/.bss section");
                }
            }

            // Third, if neither executable nor writable, handle .rodata sections
            else if sec_name.starts_with(RODATA_PREFIX) {
                if let Some(name) = sec_name.get(RODATA_PREFIX.len() ..) {
                    let demangled = demangle(name).to_string();

                    if let Some((ref rp_ref, ref mut rp)) = read_only_pages_locked {
                        // here: we're ready to copy the rodata section to the proper address
                        let dest_vaddr = rp.address_at_offset(rodata_offset)
                            .ok_or_else(|| "BUG: rodata_offset wasn't within rodata_mapped_pages")?;
                        let dest_slice: &mut [u8]  = rp.as_slice_mut(rodata_offset, sec_size)?;
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
                        
                        loaded_sections.insert(
                            shndx, 
                            Arc::new(LoadedSection::new(
                                SectionType::Rodata,
                                demangled,
                                Arc::clone(rp_ref),
                                rodata_offset,
                                dest_vaddr,
                                sec_size,
                                global_sections.contains(&shndx),
                                new_crate_weak_ref.clone(),
                            ))
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

            // Fourth, if neither executable nor writable nor .rodata, handle the `.gcc_except_table` section
            else if sec_name == GCC_EXCEPT_TABLE_NAME {
                // The gcc_except_table section is read-only, so we put it in the .rodata pages
                if let Some((ref rp_ref, ref mut rp)) = read_only_pages_locked {
                    // here: we're ready to copy the rodata section to the proper address
                    let dest_vaddr = rp.address_at_offset(rodata_offset)
                        .ok_or_else(|| "BUG: rodata_offset wasn't within rodata_mapped_pages")?;
                    let dest_slice: &mut [u8]  = rp.as_slice_mut(rodata_offset, sec_size)?;
                    match sec.get_data(&elf_file) {
                        Ok(SectionData::Undefined(sec_data)) => dest_slice.copy_from_slice(sec_data),
                        Ok(SectionData::Empty) => {
                            for b in dest_slice {
                                *b = 0;
                            }
                        },
                        _ => {
                            error!("load_crate_sections(): Couldn't get section data for .gcc_except_table section [{}] {}: {:?}", shndx, sec_name, sec.get_data(&elf_file));
                            return Err("couldn't get section data in .gcc_except_table section");
                        }
                    }

                    loaded_sections.insert(
                        shndx, 
                        Arc::new(LoadedSection::new(
                            SectionType::GccExceptTable,
                            sec_name.to_string(),
                            Arc::clone(rp_ref),
                            rodata_offset,
                            dest_vaddr,
                            sec_size,
                            false, // .gcc_except_table section is not globally visible,
                            new_crate_weak_ref.clone(),
                        ))
                    );

                    rodata_offset += round_up_power_of_two(sec_size, sec_align);
                }
                else {
                    return Err("no rodata_pages were allocated when handling .gcc_except_table");
                }
            }

            // Fifth, if neither executable nor writable nor .rodata nor .gcc_except_table, handle the `.eh_frame` section
            else if sec_name == EH_FRAME_NAME {
                // The eh_frame section is read-only, so we put it in the .rodata pages
                if let Some((ref rp_ref, ref mut rp)) = read_only_pages_locked {
                    // here: we're ready to copy the rodata section to the proper address
                    let dest_vaddr = rp.address_at_offset(rodata_offset)
                        .ok_or_else(|| "BUG: rodata_offset wasn't within rodata_mapped_pages")?;
                    let dest_slice: &mut [u8]  = rp.as_slice_mut(rodata_offset, sec_size)?;
                    match sec.get_data(&elf_file) {
                        Ok(SectionData::Undefined(sec_data)) => dest_slice.copy_from_slice(sec_data),
                        Ok(SectionData::Empty) => {
                            for b in dest_slice {
                                *b = 0;
                            }
                        },
                        _ => {
                            error!("load_crate_sections(): Couldn't get section data for .eh_frame section [{}] {}: {:?}", shndx, sec_name, sec.get_data(&elf_file));
                            return Err("couldn't get section data in .eh_frame section");
                        }
                    }

                    loaded_sections.insert(
                        shndx, 
                        Arc::new(LoadedSection::new(
                            SectionType::EhFrame,
                            sec_name.to_string(),
                            Arc::clone(rp_ref),
                            rodata_offset,
                            dest_vaddr,
                            sec_size,
                            false, // .eh_frame section is not globally visible,
                            new_crate_weak_ref.clone(),
                        ))
                    );

                    rodata_offset += round_up_power_of_two(sec_size, sec_align);
                }
                else {
                    return Err("no rodata_pages were allocated when handling .eh_frame");
                }
            }

            // Finally, any other section type is considered unhandled, so return an error!
            else {
                // .debug_* sections are handled separately, and are loaded on demand.
                if sec_name.starts_with(".debug") {
                    continue;
                }
                error!("unhandled section [{}], name: {}, sec: {:?}", shndx, sec_name, sec);
                return Err("load_crate_sections(): section with unhandled type, name, or flags!");
            }
        }

        // set the new_crate's section-related lists, since we didn't do it earlier
        {
            let mut new_crate_mut = new_crate.lock_as_mut()
                .ok_or_else(|| "BUG: load_crate_sections(): couldn't get exclusive mutable access to new_crate")?;
            new_crate_mut.sections = loaded_sections;
            new_crate_mut.global_sections = global_sections;
            new_crate_mut.data_sections = data_sections;
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
        temp_backup_namespace: Option<&CrateNamespace>,
        kernel_mmi_ref: &MmiRef,
        verbose_log: bool
    ) -> Result<(), &'static str> {
        let mut new_crate = new_crate_ref.lock_as_mut()
            .ok_or("BUG: perform_relocations(): couldn't get exclusive mutable access to new_crate")?;
        if verbose_log { debug!("=========== moving on to the relocations for crate {} =========", new_crate.crate_name); }
        let symtab = find_symbol_table(&elf_file)?;

        // Fix up the sections that were just loaded, using proper relocation info.
        // Iterate over every non-zero relocation section in the file
        for sec in elf_file.section_iter().filter(|sec| sec.get_type() == Ok(ShType::Rela) && sec.size() != 0) {
            use xmas_elf::sections::SectionData::Rela64;
            if verbose_log { 
                trace!("Found Rela section name: {:?}, type: {:?}, target_sec_index: {:?}", 
                sec.get_name(&elf_file), sec.get_type(), sec.info()); 
            }

            // Debug sections are handled separately
            if let Ok(name) = sec.get_name(&elf_file) {
                if name.starts_with(".rela.debug")         // ignore debug special sections for now
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
            let target_sec = new_crate.sections.get(&target_sec_shndx).ok_or_else(|| {
                error!("ELF file error: target section was not loaded for Rela section {:?}!", sec.get_name(&elf_file));
                "target section was not loaded for Rela section"
            })?; 
            
            let mut target_sec_dependencies: Vec<StrongDependency> = Vec::new();
            #[cfg(internal_deps)]
            let mut target_sec_internal_dependencies: Vec<InternalDependency> = Vec::new();
            {
                let mut target_sec_mapped_pages = target_sec.mapped_pages.lock();

                // iterate through each relocation entry in the relocation array for the target_sec
                for rela_entry in rela_array {
                    if verbose_log { 
                        trace!("      Rela64 offset: {:#X}, addend: {:#X}, symtab_index: {}, type: {:#X}", 
                            rela_entry.get_offset(), rela_entry.get_addend(), rela_entry.get_symbol_table_index(), rela_entry.get_type());
                    }

                    use xmas_elf::symbol_table::Entry;
                    let source_sec_entry = &symtab[rela_entry.get_symbol_table_index() as usize];
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
                    let source_sec = match new_crate.sections.get(&source_sec_shndx) {
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
                                    source_sec_name.get(DATARELRO.len() ..).ok_or("Couldn't get name of .data.rel.ro. section")?
                                } else {
                                    source_sec_name
                                };
                                let demangled = demangle(source_sec_name).to_string();

                                // search for the symbol's demangled name in the kernel's symbol map
                                self.get_symbol_or_load(&demangled, temp_backup_namespace, kernel_mmi_ref, verbose_log)
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

                    let relocation_entry = RelocationEntry::from_elf_relocation(rela_entry);
                    write_relocation(
                        relocation_entry,
                        &mut target_sec_mapped_pages,
                        target_sec.mapped_pages_offset,
                        source_sec.start_address(),
                        verbose_log
                    )?;

                    if source_and_target_in_same_crate {
                        // We keep track of relocation information so that we can be aware of and faithfully reconstruct 
                        // inter-section dependencies even within the same crate.
                        // This is necessary for doing a deep copy of the crate in memory, 
                        // without having to re-parse that crate's ELF file (and requiring the ELF file to still exist)
                        #[cfg(internal_deps)]
                        target_sec_internal_dependencies.push(InternalDependency::new(relocation_entry, source_sec_shndx))
                    }
                    else {
                        // tell the source_sec that the target_sec is dependent upon it
                        let weak_dep = WeakDependent {
                            section: Arc::downgrade(&target_sec),
                            relocation: relocation_entry,
                        };
                        source_sec.inner.write().sections_dependent_on_me.push(weak_dep);
                        
                        // tell the target_sec that it has a strong dependency on the source_sec
                        let strong_dep = StrongDependency {
                            section: Arc::clone(&source_sec),
                            relocation: relocation_entry,
                        };
                        target_sec_dependencies.push(strong_dep);          
                    }
                }
            }

            // add the target section's dependencies and relocation details all at once
            {
                let mut target_sec_inner = target_sec.inner.write();
                target_sec_inner.sections_i_depend_on.append(&mut target_sec_dependencies);
                #[cfg(internal_deps)]
                target_sec_inner.internal_dependencies.append(&mut target_sec_internal_dependencies);
            }
        }
        // here, we're done with handling all the relocations in this entire crate


        // We need to remap each section's mapped pages with the proper permission bits, 
        // since we initially mapped them all as writable.
        if let Some(ref tp) = new_crate.text_pages { 
            tp.0.lock().remap(&mut kernel_mmi_ref.lock().page_table, TEXT_SECTION_FLAGS)?;
        }
        if let Some(ref rp) = new_crate.rodata_pages {
            rp.0.lock().remap(&mut kernel_mmi_ref.lock().page_table, RODATA_SECTION_FLAGS)?;
        }
        // data/bss sections are already mapped properly, since they're supposed to be writable


        // By default, we can safely remove the metadata for all private (non-global) .rodata sections
        // that do not have any strong dependencies (its `sections_i_depend_on` list is empty).
        // If you want all sections to be kept, e.g., for debugging, you can set the below cfg option.
        #[cfg(not(keep_private_rodata))] 
        {
            new_crate.sections.retain(|_shndx, sec| {
                let should_remove = !sec.global 
                    && sec.get_type() == SectionType::Rodata
                    && sec.inner.read().sections_i_depend_on.is_empty();
                
                // For an element to be removed, this closure should return `false`.
                !should_remove
            });
        }

        Ok(())
    }

    
    /// Adds the given symbol to this namespace's symbol map.
    /// If the symbol already exists in the symbol map, this replaces the existing symbol with the new one, warning if they differ in size.
    /// Returns true if the symbol was added, and false if it already existed and thus was merely replaced.
    fn add_symbol(
        existing_symbol_map: &mut SymbolMap,
        new_section_key: String,
        new_section: &StrongSectionRef,
        log_replacements: bool,
    ) -> bool {
        match existing_symbol_map.entry(new_section_key.into()) {
            qp_trie::Entry::Occupied(mut old_val) => {
                if log_replacements {
                    if let Some(old_sec) = old_val.get().upgrade() {
                        // debug!("       add_symbol(): replacing section: old: {:?}, new: {:?}", old_sec, new_section);
                        if new_section.size() != old_sec.size() {
                            warn!("Unexpectedly replacing differently-sized section: old: ({}B) {:?}, new: ({}B) {:?}", old_sec.size(), old_sec.name, new_section.size(), new_section.name);
                        } 
                        else {
                            warn!("Replacing new symbol already present: old {:?}, new: {:?}", old_sec.name, new_section.name);
                        }
                    }
                }
                old_val.insert(Arc::downgrade(new_section));
                false
            }
            qp_trie::Entry::Vacant(new_entry) => {
                if log_replacements { 
                    debug!("         add_symbol(): Adding brand new symbol: new: {:?}", new_section);
                }
                new_entry.insert(Arc::downgrade(new_section));
                true
            }
        }
    }

    /// Adds only *global* symbols in the given `sections` iterator to this namespace's symbol map,
    /// 
    /// If a symbol already exists in the symbol map, this replaces the existing symbol but does not count it as a newly-added one.
    /// 
    /// Returns the number of *new* unique symbols added.
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
    /// but only sections that are *global* AND for which the given `filter_func` returns true. 
    /// 
    /// If a symbol already exists in the symbol map, this replaces the existing symbol but does not count it as a newly-added one.
    /// 
    /// Returns the number of *new* unique symbols added.
    fn add_symbols_filtered<'a, I, F>(
        &self, 
        sections: I,
        filter_func: F,
        log_replacements: bool,
    ) -> usize
        where I: IntoIterator<Item = &'a StrongSectionRef>,
              F: Fn(&LoadedSection) -> bool
    {
        let mut existing_map = self.symbol_map.lock();

        // add all the global symbols to the symbol map, in a way that lets us inspect/log each one
        let mut count = 0;
        for sec in sections.into_iter() {
            let condition = filter_func(&sec) && sec.global;
            if condition {
                // trace!("add_symbols_filtered(): adding symbol {:?}", sec);
                let added = CrateNamespace::add_symbol(&mut existing_map, sec.name.clone(), sec, log_replacements);
                if added {
                    count += 1;
                }
            }
            // else {
            //     trace!("add_symbols_filtered(): skipping symbol {:?}", sec);
            // }
        }
        
        count
    }

    
    /// Finds the crate that contains the given `VirtualAddress` in its loaded code.
    /// 
    /// By default, only executable sections (`.text`) are searched, since typically the only use case 
    /// for this function is to search for an instruction pointer (program counter) address.
    /// However, if `search_all_section_types` is `true`, both the read-only and read-write sections
    /// will be included in the search, e.g., `.rodata`, `.data`, `.bss`. 
    /// 
    /// # Usage
    /// This is mostly useful for printing symbol names for a stack trace (backtrace).
    /// It is also similar in functionality to the tool `addr2line`, 
    /// but gives the section itself rather than the line of code.
    /// 
    /// # Locking
    /// This can obtain the lock on every crate and every section, 
    /// so to avoid deadlock, please ensure that the caller task does not hold any such locks.
    /// It does *not* need to obtain locks on the underlying `MappedPages` regions.
    /// 
    /// # Note
    /// This is a slow procedure because, in the worst case,
    /// it will iterate through **every** loaded crate in this namespace (and its recursive namespace).
    pub fn get_crate_containing_address(
        &self, 
        virt_addr: VirtualAddress, 
        search_all_section_types: bool,
    ) -> Option<StrongCrateRef> {

        // A closure to test whether the given `crate_ref` contains the `virt_addr`.
        let crate_contains_vaddr = |crate_ref: &StrongCrateRef| {
            let krate = crate_ref.lock_as_ref();
            if let Some(ref tp) = krate.text_pages {
                if tp.1.contains(&virt_addr) { 
                    return true;
                }
            }
            if search_all_section_types {
                if let Some(ref rp) = krate.rodata_pages {
                    if rp.1.contains(&virt_addr) {
                        return true;
                    }
                }
                if let Some(ref dp) = krate.data_pages {
                    if dp.1.contains(&virt_addr) {
                        return true;
                    }
                }
            }
            false
        };
        
        let mut found_crate = None;

        // Here, we didn't find the symbol when searching from the starting crate, 
        // so perform a brute-force search of all crates in this namespace (recursively).
        self.for_each_crate(true, |_crate_name, crate_ref| {
            if crate_contains_vaddr(crate_ref) {
                found_crate = Some(crate_ref.clone());
                false // stop iterating, we've found it!
            }
            else {
                true // keep searching
            }
        });

        found_crate
    }


    /// Finds the section that contains the given `VirtualAddress` in its loaded code.
    /// 
    /// By default, only executable sections (`.text`) are searched, since typically the only use case 
    /// for this function is to search for an instruction pointer (program counter) address.
    /// However, if `search_all_section_types` is `true`, both the read-only and read-write sections
    /// will be included in the search, e.g., `.rodata`, `.data`, `.bss`. 
    /// 
    /// # Usage
    /// This is mostly useful for printing symbol names for a stack trace (backtrace).
    /// It is also similar in functionality to the tool `addr2line`, 
    /// but gives the section itself rather than the line of code.
    /// 
    /// # Locking
    /// This can obtain the lock on every crate and every section, 
    /// so to avoid deadlock, please ensure that the caller task does not hold any such locks.
    /// 
    /// # Note
    /// This is a slow procedure because, in the worst case,
    /// it will iterate through **every** section in **every** loaded crate 
    /// in this namespace (and its recursive namespace),
    /// not just the publicly-visible (global) sections. 
    pub fn get_section_containing_address(
        &self, 
        virt_addr: VirtualAddress, 
        search_all_section_types: bool,
    ) -> Option<(StrongSectionRef, usize)> {

        // First, we find the crate that contains the address, then later we narrow it down.
        let containing_crate = self.get_crate_containing_address(virt_addr, search_all_section_types)?;
        let crate_locked = containing_crate.lock_as_ref();

        // Second, we find the section in that crate that contains the address.
        for sec in crate_locked.sections.values() {
            // .text sections are always included, other sections are included if requested.
            let eligible_section = sec.typ == SectionType::Text || search_all_section_types;
            
            // If the section's address bounds contain the address, then we've found it.
            // Only a single section can contain the address, so it's safe to stop once we've found a match.
            if eligible_section && sec.address_range.contains(&virt_addr) {
                let offset = virt_addr.value() - sec.start_address().value();
                return Some((sec.clone(), offset));
            }
        }
        None
    }

    /// Like [`get_symbol()`](#method.get_symbol), but also returns the exact `CrateNamespace` where the symbol was found.
    pub fn get_symbol_and_namespace(&self, demangled_full_symbol: &str) -> Option<(WeakSectionRef, &CrateNamespace)> {
        let weak_symbol = self.symbol_map.lock().get_str(demangled_full_symbol).cloned();
        weak_symbol.map(|sym| (sym, self))
            // search the recursive namespace if the symbol cannot be found in this namespace
            .or_else(|| self.recursive_namespace.as_ref().and_then(|rns| rns.get_symbol_and_namespace(demangled_full_symbol)))
    }

    /// A convenience function that returns a weak reference to the `LoadedSection`
    /// that matches the given name (`demangled_full_symbol`), 
    /// if it exists in this namespace's or its recursive namespace's symbol map.
    /// Otherwise, it returns None if the symbol does not exist.
    fn get_symbol_internal(&self, demangled_full_symbol: &str) -> Option<WeakSectionRef> {
        self.get_symbol_and_namespace(demangled_full_symbol).map(|(sym, _ns)| sym)
    }

    /// Finds the corresponding `LoadedSection` reference for the given fully-qualified symbol string.
    /// Searches this namespace first, and then its recursive namespace as well.
    pub fn get_symbol(&self, demangled_full_symbol: &str) -> WeakSectionRef {
        self.get_symbol_internal(demangled_full_symbol).unwrap_or_default()
    }


    /// Finds the corresponding `LoadedSection` reference for the given fully-qualified symbol string,
    /// similar to the simpler function `get_symbol()`, but takes the additional step of trying to 
    /// automatically find and/or load the crate containing that symbol 
    /// (and does so recursively for any of its crate dependencies).
    /// 
    /// (1) First, it recursively searches this namespace's and its recursive namespaces' symbol maps, 
    ///     and returns the symbol if already loaded.
    /// 
    /// (2) Second, if the symbol is missing from this namespace, it looks in the `temp_backup_namespace`. 
    ///     If we find it there, then we add that symbol and its containing crate as a shared crate in this namespace.
    /// 
    /// (3) Third, if this namespace has `fuzzy_symbol_matching` enabled, it searches the backup namespace
    ///     for symbols that match the given `demangled_full_symbol` without the hash suffix. 
    /// 
    /// (4) Fourth, if the missing symbol isn't in the backup namespace either, 
    ///     try to load its containing crate from the object file. 
    ///     This can only be done for symbols that have a leading crate name, such as "my_crate::foo";
    ///     if a symbol was given the `no_mangle` attribute, then we will not be able to find it,
    ///     and that symbol's containing crate should be manually loaded before invoking this. 
    /// 
    /// 
    /// # Arguments
    /// * `demangled_full_symbol`: a fully-qualified symbol string, e.g., "my_crate::MyStruct::foo::h843a9ea794da0c24".
    /// * `temp_backup_namespace`: the `CrateNamespace` that should be temporarily searched (just during this call)
    ///   for the missing symbol.
    ///   If `temp_backup_namespace` is `None`, then only this namespace (and its recursive namespaces) will be searched.
    /// * `kernel_mmi_ref`: a reference to the kernel's `MemoryManagementInfo`, which must not be locked.
    pub fn get_symbol_or_load(
        &self, 
        demangled_full_symbol: &str, 
        temp_backup_namespace: Option<&CrateNamespace>, 
        kernel_mmi_ref: &MmiRef,
        verbose_log: bool
    ) -> WeakSectionRef {
        // First, see if the section for the given symbol is already available and loaded
        // in either this namespace or its recursive namespace
        if let Some(weak_sec) = self.get_symbol_internal(demangled_full_symbol) {
            return weak_sec;
        }

        // If not, our second option is to check the temp_backup_namespace to see if that namespace already has the section we want.
        // If we can find it there, that saves us the effort of having to load the crate again from scratch.
        if let Some(backup) = temp_backup_namespace {
            // info!("Symbol \"{}\" not initially found, attempting to load it from backup namespace {:?}", 
            //     demangled_full_symbol, backup.name);
            if let Some(sec) = self.get_symbol_from_backup_namespace(demangled_full_symbol, backup, false, verbose_log) {
                return Arc::downgrade(&sec);
            }
        }

        // Try to fuzzy match the symbol to see if a single match for it has already been loaded into the backup namespace.
        // This is basically the same code as the above temp_backup_namespace conditional, but checks to ensure there aren't multiple fuzzy matches.
        if self.fuzzy_symbol_matching {
            if let Some(backup) = temp_backup_namespace {
                // info!("Symbol \"{}\" not initially found, attempting to load it from backup namespace {:?}", 
                //     demangled_full_symbol, backup.name);
                if let Some(sec) = self.get_symbol_from_backup_namespace(demangled_full_symbol, backup, true, verbose_log) {
                    return Arc::downgrade(&sec);
                }
            }
        }

        // Finally, try to load the crate containing the missing symbol.
        if let Some(weak_sec) = self.load_crate_for_missing_symbol(demangled_full_symbol, temp_backup_namespace, kernel_mmi_ref, verbose_log) {
            weak_sec
        } else {
            #[cfg(not(loscd_eval))]
            error!("Symbol \"{}\" not found. Try loading the specific crate manually first.", demangled_full_symbol);
            Weak::default() // same as returning None, since it must be upgraded to an Arc before being used
        }
    }


    /// Looks for the given `demangled_full_symbol` in the `temp_backup_namespace` and returns a reference to the matching section. 
    /// 
    /// This is the second and third attempts to find a symbol within [`get_symbol_or_load()`](#method.get_symbol_or_load).
    fn get_symbol_from_backup_namespace(
        &self,
        demangled_full_symbol: &str,
        temp_backup_namespace: &CrateNamespace,
        fuzzy_matching: bool,
        verbose_log: bool,
    ) -> Option<StrongSectionRef> {
        let mut _fuzzy_matched_symbol_name: Option<String> = None;

        let (weak_sec, _found_in_ns) = if !fuzzy_matching {
            // use exact (non-fuzzy) matching
            temp_backup_namespace.get_symbol_and_namespace(demangled_full_symbol)?
        } else {
            // use fuzzy matching (ignoring the symbol hash suffix)
            let fuzzy_matches = temp_backup_namespace.find_symbols_starting_with_and_namespace(LoadedSection::section_name_without_hash(demangled_full_symbol));
            match fuzzy_matches.as_slice() {
                [(sec_name, weak_sec, _found_in_ns)] => {
                    _fuzzy_matched_symbol_name = Some(sec_name.clone());
                    (weak_sec.clone(), *_found_in_ns)
                }
                fuzzy_matches => {
                    warn!("Cannot resolve dependency because there are {} fuzzy matches for symbol {:?} in backup namespace {:?}\n\t{:?}",
                        fuzzy_matches.len(), 
                        demangled_full_symbol, 
                        temp_backup_namespace.name, 
                        fuzzy_matches.into_iter().map(|tup| &tup.0).collect::<Vec<_>>()
                    );
                    return None;
                }
            }
        };
        let sec = weak_sec.upgrade().or_else(|| {
            error!("Found matching symbol \"{}\" in backup namespace, but unexpectedly couldn't upgrade it to a strong section reference!", demangled_full_symbol);
            None
        })?;

        // Here, we found the matching section in the temp_backup_namespace.
        let parent_crate_ref = { 
            sec.parent_crate.upgrade().or_else(|| {
                error!("BUG: Found symbol \"{}\" in backup namespace, but unexpectedly couldn't get its parent crate!", demangled_full_symbol);
                None
            })?
        };
        let parent_crate_name = {
            let parent_crate = parent_crate_ref.lock_as_ref();
            // Here, there is a misguided potential for optimization: add all symbols from the parent_crate into the current namespace.
            // While this would save lookup/loading time if future symbols were needed from this crate,
            // we *cannot* do this because it violates the expectations of certain namespaces. 
            // For example, some namespaces may want to use just *one* symbol from another namespace's crate, not all of them. 
            // Thus, we just add the one symbol for `sec` to this namespace.
            self.add_symbols(Some(sec.clone()).iter(), verbose_log);
            parent_crate.crate_name.clone()
        };
        
        #[cfg(not(loscd_eval))]
        info!("Symbol {:?} not initially found, using {}symbol {} from crate {:?} in backup namespace {:?} in new namespace {:?}",
            demangled_full_symbol, 
            if fuzzy_matching { "fuzzy-matched " } else { "" },
            _fuzzy_matched_symbol_name.unwrap_or_default(),
            parent_crate_name,
            _found_in_ns.name,
            self.name
        );

        // We add a shared reference to that section's parent crate to this namespace as well, 
        // to prevent that crate from being dropped while this namespace still relies on it.
        self.crate_tree.lock().insert(parent_crate_name.into(), parent_crate_ref);
        return Some(sec);
    }


    /// Attempts to find and load the crate containing the given `demangled_full_symbol`. 
    /// If successful, the new crate is loaded into this `CrateNamespace` and the symbol's section is returned.
    /// 
    /// If this namespace does not contain any matching crates, its recursive namespaces are searched as well.
    /// 
    /// This approach only works for mangled symbols that contain a crate name, such as "my_crate::foo". 
    /// If "foo()" was marked no_mangle, then we don't know which crate to load because there is no "my_crate::" prefix before it.
    /// 
    /// This is the final attempt to find a symbol within [`get_symbol_or_load()`](#method.get_symbol_or_load).
    fn load_crate_for_missing_symbol(
        &self,
        demangled_full_symbol: &str,
        temp_backup_namespace: Option<&CrateNamespace>,
        kernel_mmi_ref: &MmiRef,
        verbose_log: bool,
    ) -> Option<WeakSectionRef> {
        // Some symbols may have multiple potential containing crates, so we try to load each one to find the missing symbol.
        for potential_crate_name in get_containing_crate_name(demangled_full_symbol) {
            let potential_crate_name = format!("{}-", potential_crate_name);
 
            // Try to find and load the missing crate object file from this namespace's directory or its recursive namespace's directory,
            // (or from the backup namespace's directory set).
            // We *do not* search recursively here since we want the new crate to be loaded into the namespace 
            // that contains its crate object file, not a higher-level namespace. 
            // Checking recursive namespaces will occur at the end of this function during the recursive call to this same function.
            let (potential_crate_file, ns_of_crate_file) = match self.method_get_crate_object_file_starting_with(&potential_crate_name) {
                Some(found) => found,
                // TODO: should we be blindly recursively searching the backup namespace's files?
                None => match temp_backup_namespace.and_then(|backup| backup.method_get_crate_object_file_starting_with(&potential_crate_name)) {
                    // do not modify the backup namespace, instead load its crate into this namespace
                    Some((crate_file_in_backup_ns, _backup_ns)) => (crate_file_in_backup_ns, self),
                    None => {
                        warn!("Couldn't find a single crate object file with prefix {:?} that may contain symbol {:?} in namespace {:?}.", 
                            potential_crate_name, demangled_full_symbol, self.name);
                        continue;
                    }
                }
            };
                          
            let potential_crate_file_path = Path::new(potential_crate_file.lock().get_absolute_path());
            // Check to make sure this crate is not already loaded into this namespace (or its recursive namespace).
            if self.get_crate(crate_name_from_path(&potential_crate_file_path)).is_some() {
                trace!("  (skipping already-loaded crate {:?})", potential_crate_file_path);
                continue;
            }
            #[cfg(not(loscd_eval))]
            info!("Symbol {:?} not initially found in namespace {:?}, attempting to load crate {:?} into namespace {:?} that may contain it.", 
                demangled_full_symbol, self.name, potential_crate_name, ns_of_crate_file.name);

            match ns_of_crate_file.load_crate(&potential_crate_file, temp_backup_namespace, kernel_mmi_ref, verbose_log) {
                Ok((_new_crate_ref, _num_new_syms)) => {
                    // try again to find the missing symbol, now that we've loaded the missing crate
                    if let Some(sec) = ns_of_crate_file.get_symbol_internal(demangled_full_symbol) {
                        return Some(sec);
                    } else {
                        // the missing symbol wasn't in this crate, continue to load the other potential containing crates.
                        trace!("Loaded symbol's containing crate {:?}, but still couldn't find the symbol {:?}.", 
                            potential_crate_file_path, demangled_full_symbol);
                    }
                }
                Err(_e) => {
                    error!("Found symbol's (\"{}\") containing crate, but couldn't load the crate file {:?}. Error: {:?}",
                        demangled_full_symbol, potential_crate_file_path, _e);
                    // We *could* return an error here, but we might as well continue on to trying to load other crates.
                }
            }
        }

        None
    }


    /// Returns a copied list of the corresponding `LoadedSection`s 
    /// with names that start with the given `symbol_prefix`.
    /// This will also search the recursive namespace's symbol map. 
    /// 
    /// This method causes allocation because it creates a copy
    /// of the matching entries in the symbol map.
    /// 
    /// # Example
    /// The symbol map contains `my_crate::foo::h843a613894da0c24` and 
    /// `my_crate::foo::h933a635894ce0f12`. 
    /// Calling `find_symbols_starting_with("my_crate::foo")` will return 
    /// a vector containing both sections, which can then be iterated through.
    pub fn find_symbols_starting_with(&self, symbol_prefix: &str) -> Vec<(String, WeakSectionRef)> { 
        let mut syms: Vec<(String, WeakSectionRef)> = self.symbol_map.lock()
            .iter_prefix_str(symbol_prefix)
            .map(|(k, v)| (String::from(k.as_str()), v.clone()))
            .collect();

        if let Some(mut syms_recursive) = self.recursive_namespace.as_ref().map(|r_ns| r_ns.find_symbols_starting_with(symbol_prefix)) {
            syms.append(&mut syms_recursive);
        }

        syms
    }


    /// Similar to `find_symbols_starting_with`, but also includes a reference to the exact `CrateNamespace`
    /// where the matching symbol was found.
    pub fn find_symbols_starting_with_and_namespace(&self, symbol_prefix: &str) -> Vec<(String, WeakSectionRef, &CrateNamespace)> { 
        let mut syms: Vec<(String, WeakSectionRef, &CrateNamespace)> = self.symbol_map.lock()
            .iter_prefix_str(symbol_prefix)
            .map(|(k, v)| (String::from(k.as_str()), v.clone(), self))
            .collect();

        if let Some(mut syms_recursive) = self.recursive_namespace.as_ref().map(|r_ns| r_ns.find_symbols_starting_with_and_namespace(symbol_prefix)) {
            syms.append(&mut syms_recursive);
        }

        syms
    }


    /// Returns a weak reference to the `LoadedSection` whose name beings with the given `symbol_prefix`,
    /// *if and only if* the symbol map only contains a single possible matching symbol.
    /// This will also search the recursive namespace's symbol map. 
    /// 
    /// # Important Usage Note
    /// To avoid greedily matching more symbols than expected, you may wish to end the `symbol_prefix` with "`::`".
    /// This may provide results more in line with the caller's expectations; see the last example below about a trailing "`::`". 
    /// This works because the delimiter between a symbol and its trailing hash value is "`::`".
    /// 
    /// # Example
    /// * The symbol map contains `my_crate::foo::h843a613894da0c24` 
    ///   and no other symbols that start with `my_crate::foo`. 
    ///   Calling `get_symbol_starting_with("my_crate::foo")` will return 
    ///   a weak reference to the section `my_crate::foo::h843a613894da0c24`.
    /// * The symbol map contains `my_crate::foo::h843a613894da0c24` and 
    ///   `my_crate::foo::h933a635894ce0f12`. 
    ///   Calling `get_symbol_starting_with("my_crate::foo")` will return 
    ///   an empty (default) weak reference, which is the same as returing None.
    /// * (Important) The symbol map contains `my_crate::foo::h843a613894da0c24` and 
    ///   `my_crate::foo_new::h933a635894ce0f12`. 
    ///   Calling `get_symbol_starting_with("my_crate::foo")` will return 
    ///   an empty (default) weak reference, which is the same as returing None,
    ///   because it will match both `foo` and `foo_new`. 
    ///   To match only `foo`, call this function as `get_symbol_starting_with("my_crate::foo::")`
    ///   (note the trailing "`::`").
    pub fn get_symbol_starting_with(&self, symbol_prefix: &str) -> WeakSectionRef { 
        self.get_symbol_starting_with_internal(symbol_prefix)
            .unwrap_or_default()
    }

    /// This is an internal version of method: [`get_symbol_starting_with()`](#method.get_symbol_starting_with) 
    /// that returns an Option to allow easier recursive use.
    fn get_symbol_starting_with_internal(&self, symbol_prefix: &str) -> Option<WeakSectionRef> { 
        // First, we see if there's a single matching symbol in this namespace. 
        let map = self.symbol_map.lock();
        let mut iter = map.iter_prefix_str(symbol_prefix).map(|tuple| tuple.1);
        let symbol_in_this_namespace = iter.next()
            .filter(|_| iter.next().is_none()) // ensure single element
            .cloned();
        
        // Second, we see if there's a single matching symbol in the recursive namespace.
        let symbol_in_recursive_namespace = self.recursive_namespace.as_ref().and_then(|r_ns| r_ns.get_symbol_starting_with_internal(symbol_prefix));

        // There can only be one matching crate across all recursive namespaces.
        symbol_in_this_namespace.xor(symbol_in_recursive_namespace)
    }

    
    /// Simple debugging function that returns the entire symbol map as a String.
    /// This includes only symbols from this namespace, and excludes symbols from recursive namespaces.
    pub fn dump_symbol_map(&self) -> String {
        use core::fmt::Write;
        let mut output: String = String::new();
        let sysmap = self.symbol_map.lock();
        match write!(&mut output, "{:?}", sysmap.keys().collect::<Vec<_>>()) {
            Ok(_) => output,
            _ => String::from("(error)"),
        }
    }

    /// Same as [`dump_symbol_map()`](#method.dump_symbol_map), 
    /// but includes symbols from recursive namespaces.
    pub fn dump_symbol_map_recursive(&self) -> String {
        let mut syms = self.dump_symbol_map();

        if let Some(ref r_ns) = self.recursive_namespace {
            let syms_recursive = r_ns.dump_symbol_map_recursive();
            syms = format!("{}\n{}", syms, syms_recursive);
        }

        syms
    }
}


/// A convenience wrapper for a set of the three possible types of `MappedPages`
/// that can be allocated and mapped for a single `LoadedCrate`. 
struct SectionPages {
    /// MappedPages that will hold any and all executable sections: `.text`
    /// and their bounds express in `VirtualAddress`es.
    executable_pages: Option<(MappedPages, Range<VirtualAddress>)>,
    /// MappedPages that will hold any and all read-only sections: `.rodata`, `.eh_frame`, `.gcc_except_table`
    /// and their bounds express in `VirtualAddress`es.
    read_only_pages: Option<(MappedPages, Range<VirtualAddress>)>,
    /// MappedPages that will hold any and all read-write sections: `.data` and `.bss`
    /// and their bounds express in `VirtualAddress`es.
    read_write_pages: Option<(MappedPages, Range<VirtualAddress>)>,
}


/// Allocates and maps memory sufficient to hold the sections that are found in the given `ElfFile`.
/// Only sections that are marked "allocated" (`ALLOC`) in the ELF object file will contribute to the mappings' sizes.
fn allocate_section_pages(elf_file: &ElfFile, kernel_mmi_ref: &MmiRef) -> Result<SectionPages, &'static str> {
    // Calculate how many bytes (and thus how many pages) we need for each of the three section types.
    //
    // Since all executable .text sections come at the beginning of the object file, we can simply find 
    // the end of the last .text section and then use it as the end bounds.
    let (exec_bytes, ro_bytes, rw_bytes): (usize, usize, usize) = {
        let mut text_max_offset = 0;
        let mut ro_bytes = 0;
        let mut rw_bytes = 0;
        for (shndx, sec) in elf_file.section_iter().enumerate() {
            let sec_flags = sec.flags();
            // Skip non-allocated sections; they don't need to be loaded into memory
            if sec_flags & SHF_ALLOC == 0 {
                continue;
            }

            // Zero-sized sections may be aliased references to the next section in the ELF file,
            // but only if they have the same offset.
            // The empty .text section at the start of each object file should be ignored. 
            let sec = if (sec.size() == 0) && (sec.get_name(elf_file) != Ok(".text")) {
                let next_sec = elf_file.section_header((shndx + 1) as u16)
                    .map_err(|_| "couldn't get next section for a zero-sized section")?;
                if next_sec.offset() == sec.offset() {
                    next_sec
                } else {
                    sec
                }
            } else {
                sec
            };

            let size = sec.size() as usize;
            let align = sec.align() as usize;
            let addend = round_up_power_of_two(size, align);

            // filter flags for ones we care about (we already checked that it's loaded (SHF_ALLOC))
            let write: bool = sec_flags & SHF_WRITE     == SHF_WRITE;
            let exec:  bool = sec_flags & SHF_EXECINSTR == SHF_EXECINSTR;
            // trace!("  Looking at sec {:?}, size {:#X}, align {:#X} --> addend {:#X}", sec.get_name(elf_file), size, align, addend);
            if exec {
                // this includes only .text sections
                text_max_offset = core::cmp::max(text_max_offset, (sec.offset() as usize) + addend);
            }
            else if write {
                // this includes both .bss and .data sections
                rw_bytes += addend;
            }
            else {
                // this includes .rodata, plus special sections like .eh_frame and .gcc_except_table
                ro_bytes += addend;
            }
        }
        (text_max_offset, ro_bytes, rw_bytes)
    };

    // Allocate contiguous virtual memory pages for each section and map them to random frames as writable.
    // We must allocate these pages separately because they will have different flags later.
    let executable_pages = if exec_bytes > 0 { Some(allocate_and_map_as_writable(exec_bytes, TEXT_SECTION_FLAGS,     kernel_mmi_ref)?) } else { None };
    let read_only_pages =  if ro_bytes   > 0 { Some(allocate_and_map_as_writable(ro_bytes,   RODATA_SECTION_FLAGS,   kernel_mmi_ref)?) } else { None };
    let read_write_pages = if rw_bytes   > 0 { Some(allocate_and_map_as_writable(rw_bytes,   DATA_BSS_SECTION_FLAGS, kernel_mmi_ref)?) } else { None };

    let range_tuple = |mp: MappedPages, size_in_bytes: usize| {
        let start = mp.start_address();
        (mp, start..(start + size_in_bytes))
    };

    Ok(SectionPages {
        executable_pages: executable_pages.map(|mp| range_tuple(mp, exec_bytes)),
        read_only_pages:  read_only_pages .map(|mp| range_tuple(mp, ro_bytes)),
        read_write_pages: read_write_pages.map(|mp| range_tuple(mp, rw_bytes)),
    })
}


/// A convenience function for allocating contiguous virtual memory pages and mapping them to random physical frames. 
/// 
/// The returned `MappedPages` will be at least as large as `size_in_bytes`, rounded up to the nearest `Page` size, 
/// and is mapped as writable along with the other specified `flags` to ensure we can copy content into it.
fn allocate_and_map_as_writable(size_in_bytes: usize, flags: EntryFlags, kernel_mmi_ref: &MmiRef) -> Result<MappedPages, &'static str> {
    let allocated_pages = allocate_pages_by_bytes(size_in_bytes).ok_or("Couldn't allocate_pages_by_bytes, out of virtual address space")?;
    kernel_mmi_ref.lock().page_table.map_allocated_pages(allocated_pages, flags | EntryFlags::PRESENT | EntryFlags::WRITABLE)
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
    let sec_inner = sec.inner.read();
	if !sec_inner.sections_dependent_on_me.is_empty() {
		debug!("{}Section \"{}\": sections dependent on me (weak dependents):", prefix, sec.name);
		for weak_dep in &sec_inner.sections_dependent_on_me {
			if let Some(wds) = weak_dep.section.upgrade() {
				let prefix = format!("{}  ", prefix); // add two spaces of indentation to the prefix
				dump_weak_dependents(&*wds, prefix);
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
pub fn find_symbol_table<'e>(elf_file: &'e ElfFile) 
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
