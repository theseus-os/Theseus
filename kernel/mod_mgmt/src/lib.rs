#![no_std]
#![feature(rustc_private)]
#![feature(const_fn)]

#[macro_use] extern crate alloc;
#[macro_use] extern crate log;
extern crate spin;
extern crate irq_safety;
extern crate xmas_elf;
extern crate memory;
extern crate multiboot2;
extern crate kernel_config;
extern crate goblin;
extern crate util;
extern crate crate_name_utils;
extern crate rustc_demangle;
extern crate owning_ref;
extern crate cow_arc;
extern crate hashbrown;
extern crate qp_trie;
extern crate root;
extern crate vfs_node;
extern crate fs_node;
extern crate path;
extern crate memfs;
extern crate hpet;
extern crate cstr_core;
extern crate by_address;

use core::ops::{DerefMut, Deref, Range};
use core::fmt;
use alloc::{
    vec::Vec,
    collections::{BTreeMap, btree_map, BTreeSet},
    string::{String, ToString},
    sync::{Arc, Weak},
};
use spin::{Mutex, Once};
use by_address::ByAddress;

use xmas_elf::{
    ElfFile,
    sections::{SectionData, ShType, SHF_WRITE, SHF_ALLOC, SHF_EXECINSTR},
};
use goblin::elf::reloc::*;

use util::round_up_power_of_two;
use memory::{MmiRef, FRAME_ALLOCATOR, MemoryManagementInfo, FrameRange, VirtualAddress, PhysicalAddress, MappedPages, EntryFlags, allocate_pages_by_bytes};
use multiboot2::BootInformation;
use metadata::{StrongCrateRef, WeakSectionRef};
use cow_arc::CowArc;
use hashbrown::HashMap;
use rustc_demangle::demangle;
use qp_trie::{Trie, wrapper::BString};
use fs_node::{FileOrDir, File, FileRef, DirRef};
use vfs_node::VFSDirectory;
use path::Path;
use memfs::MemFile;
use crate_name_utils::{get_containing_crate_name, replace_containing_crate_name};


pub mod elf_executable;
pub mod parse_nano_core;
pub mod metadata;
pub mod dependency;

use self::metadata::*;
use self::dependency::*;


/// The name of the directory that contains all of the CrateNamespace files.
pub const NAMESPACES_DIRECTORY_NAME: &'static str = "namespaces";

/// The initial `CrateNamespace` that all kernel crates are added to by default.
static INITIAL_KERNEL_NAMESPACE: Once<Arc<CrateNamespace>> = Once::new();

/// Returns a reference to the default kernel namespace, 
/// which must exist because it contains the initially-loaded kernel crates. 
/// Returns None if the default namespace hasn't yet been initialized.
pub fn get_initial_kernel_namespace() -> Option<&'static Arc<CrateNamespace>> {
    INITIAL_KERNEL_NAMESPACE.try()
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
    let default_app_namespace_name = CrateType::Application.namespace_name().to_string(); // this will be "_applications"
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


/// `.text` sections are read-only and executable.
const TEXT_SECTION_FLAGS:     EntryFlags = EntryFlags::PRESENT;
/// `.rodata` sections are read-only and non-executable.
const RODATA_SECTION_FLAGS:   EntryFlags = EntryFlags::from_bits_truncate(EntryFlags::PRESENT.bits() | EntryFlags::NO_EXECUTE.bits());
/// `.data` and `.bss` sections are read-write and non-executable.
const DATA_BSS_SECTION_FLAGS: EntryFlags = EntryFlags::from_bits_truncate(EntryFlags::PRESENT.bits() | EntryFlags::NO_EXECUTE.bits() | EntryFlags::WRITABLE.bits());


/// Initializes the module management system based on the bootloader-provided modules, 
/// and creates and returns the default `CrateNamespace` for kernel crates.
pub fn init(boot_info: &BootInformation, kernel_mmi: &mut MemoryManagementInfo) -> Result<&'static Arc<CrateNamespace>, &'static str> {
    let (_namespaces_dir, default_kernel_namespace_dir) = parse_bootloader_modules_into_files(boot_info, kernel_mmi)?;
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
    boot_info: &BootInformation, 
    kernel_mmi: &mut MemoryManagementInfo
) -> Result<(DirRef, NamespaceDir), &'static str> {

    // create the top-level directory to hold all default namespaces
    let namespaces_dir = VFSDirectory::new(NAMESPACES_DIRECTORY_NAME.to_string(), root::get_root())?;

    // a map that associates a prefix string (e.g., "sse" in "ksse#crate.o") to a namespace directory of object files 
    let mut prefix_map: BTreeMap<String, NamespaceDir> = BTreeMap::new();

    let fa = FRAME_ALLOCATOR.try().ok_or("Couldn't get Frame Allocator")?;

    // Closure to create the directory for a new namespace.
    let create_dir = |dir_name: &str| -> Result<NamespaceDir, &'static str> {
        VFSDirectory::new(dir_name.to_string(), &namespaces_dir).map(|d| NamespaceDir(d))
    };

    for m in boot_info.module_tags() {
        let size_in_bytes = (m.end_address() - m.start_address()) as usize;
        let frames = FrameRange::from_phys_addr(PhysicalAddress::new(m.start_address() as usize)?, size_in_bytes);
        let (crate_type, prefix, file_name) = CrateType::from_module_name(m.name())?;
        let dir_name = format!("{}{}", prefix, crate_type.namespace_name());
        let name = String::from(file_name);

        let pages = allocate_pages_by_bytes(size_in_bytes).ok_or("Couldn't allocate virtual pages for bootloader module area")?;
        let mp = kernel_mmi.page_table.map_allocated_pages_to(
            pages, 
            frames, 
            EntryFlags::PRESENT, // we never need to write to bootloader-provided modules
            fa.lock().deref_mut()
        )?;

        // debug!("Module: {:?}, size {}, mp: {:?}", name, size_in_bytes, mp);

        let create_file = |dir: &DirRef| {
            MemFile::from_mapped_pages(mp, name, size_in_bytes, dir)
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
        prefix_map.remove(CrateType::Kernel.namespace_name()).ok_or("BUG: no default namespace found")?,
    ))
}


/// An object that can be converted into a crate object file.
/// We use an enum rather than implement `TryInto` because we need additional information
/// to resolve a prefix path.
pub enum IntoCrateObjectFile {
    /// A direct reference to the crate object file. This will be used as-is. 
    File(FileRef),
    /// An absolute path that points to the crate object file. 
    AbsolutePath(Path),
    /// A string prefix that will be used to search for the crate object file in the namespace.
    /// This must be able to uniquely identify a single crate object file in the namespace directory (recursively searched). 
    Prefix(String),
}

/// A list of one or more `SwapRequest`s that is used by the `swap_crates` function.
pub type SwapRequestList = Vec<SwapRequest>;
  

/// This struct is used to specify the details of a crate-swapping operation,
/// in which an "old" crate is replaced with a "new" crate that is then used in place of that old crate. 
/// The old crate is removed from its old `CrateNamespace` while the new crate 
/// is added to the new `CrateNamespace`; typically the old and new namespaces are the same. 
/// 
/// # Important Note: Reexporting Symbols
/// When swapping out an old crate, the new crate must satisfy all of the dependencies
/// that other crates had on that old crate. 
/// However, the swapping procedure will completely remove the old crate and its symbols from its containing `CrateNamespace`,
/// so it may be useful to re-expose or "mirror" the new crate's sections as symbols with names 
/// that match the relevant sections from the old crate.
/// This satisfies other crates' dependenices on the old crate while allowing the new crate to exist normally,
/// which means that the new crate's symbols appear both as those from the new crate itself 
/// in addition to those from the old crate that was replaced. 
/// To do this, set the `reexport_new_symbols_as_old` option to `true`.
/// 
/// However, **enabling this option is potentially dangerous**, as you must take responsibility 
/// for ensuring that the new crate can safely and correctly replace the old crate, 
/// and that using the new crate in place of the old crate will not break any other dependent crates.
/// This will necessarily ignore the hashes when matching symbols across the old and new crate, 
/// and hashes are how Theseus ensures that different crates and functions are linked to the correct version of each other.
/// However, this does make the swapping operation much simpler to use, 
/// because you will likely need to swap far fewer crates at once, 
/// and as a convenience bonus, you don't have to specify the exact versions (hashes) of each crate.
/// 
/// ## Example of Reexporting Symbols
/// There exists a function `drivers::init::h...` in the `drivers` crate 
/// that depends on the function `keyboard::init::hABC` in the `keyboard` crate.
/// We call `swap_crates()` to replace the `keyboard` crate with a new crate called `keyboard_new`.
/// 
/// If `reexport_new_symbols_as_old` is `true`, the new `keyboard_new` crate's function called `keyboard_new::init::hDEF`
/// will now appear in the namespace's symbol map twice: 
/// 1. in its normal form `keyboard_new::init::hDEF`, which is the normal behavior when loading a crate, and
/// 2. in its reexported form `keyboard::init::hABC`, which exactly matches the function from the old `keyboard` crate.
/// 
/// In this way, the single function in the new crate `keyboard_new` appears twice in the symbol map
/// under different names, allowing it to fulfill dependencies on both the old crate and the new crate.
/// In general, this option is not needed. 
/// 
#[derive(Eq, PartialEq, Hash)]
pub struct SwapRequest {
    // Note: the usage of `ByAddress` is to allow us to hash and compare reference types like Arc
    //       to make sure that they point to the same file rather than having the same actual contents.
    //       It makes hashing extremely cheap and fast!

    /// The full name of the old crate that will be replaced by the new crate.
    /// This will be used to search the `CrateNamespace` to find an existing `LoadedCrate`.
    /// The `SwapRequest` constructor ensures this is fully-qualified (includes a hash value)
    /// and is unique within the `old_namespace`.
    old_crate_name: String,
    /// The `CrateNamespace` that contains the given old crate, 
    /// from which that old crate and its symbols should be removed. 
    old_namespace: ByAddress<Arc<CrateNamespace>>,
    /// The object file for the new crate that will replace the old crate. 
    new_crate_object_file: ByAddress<FileRef>,
    /// The `CrateNamespace` into which the replacement new crate and its symbols should be loaded.
    /// Typically, this is the same namespace as the `old_namespace`.
    new_namespace: ByAddress<Arc<CrateNamespace>>,
    /// Whether to expose the new crate's sections with symbol names that match those from the old crate.
    /// For more details, see the above docs for this struct.
    reexport_new_symbols_as_old: bool,
}
impl fmt::Debug for SwapRequest {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("SwapRequest")
            .field("old_crate", &self.old_crate_name)
            .field("old_namespace", &self.old_namespace.name)
            .field("new_crate", &self.new_crate_object_file.try_lock()
                .map(|f| f.get_absolute_path())
                .unwrap_or_else(|| format!("<Locked>"))
            )
            .field("new_namespace", &self.new_namespace.name)
            .field("reexport_symbols", &self.reexport_new_symbols_as_old)
            .finish()
    }
}
impl SwapRequest {
    /// Create a new `SwapRequest` that, when given to `swap_crates()`, 
    /// will swap out the given old crate and replace it with the given new crate,
    /// and optionally re-export the new crate's symbols.
    /// 
    /// # Arguments
    /// * `old_crate_name`: the name of the old crate that should be unloaded and removed from the `old_namespace`. 
    ///    An `old_crate_name` of `None` signifies there is no old crate to be removed,
    ///    and that only a new crate will be added.
    /// 
    ///    Note that `old_crate_name` can be any string prefix, so long as it can uniquely identify a crate object file
    ///    in the given `old_namespace` or any of its recursive namespaces. 
    ///    Thus, to be more accurate, it is wise to specify a full crate name with hash, e.g., "my_crate-<hash>".
    /// 
    /// * `old_namespace`: the `CrateNamespace` that contains the old crate;
    ///    that old crate and its symbols will be removed from this namespace. 
    /// 
    ///    If `old_crate_name` is `None`, then `old_namespace` should be the same namespace 
    ///    as the `this_namespace` argument that `swap_crates()` is invoked with. 
    /// 
    /// * `new_crate_object_file`: a type that can be converted into a crate object file.
    ///    This can either be a direct reference to the file, an absolute `Path` that points to the file,
    ///    or a prefix string used to find the file in the new namespace's directory of crate object files. 
    /// 
    /// * `new_namespace`: the `CrateNamespace` to which the new crate will be loaded and its symbols added.
    ///    If `None`, then the new crate will be loaded into the `old_namespace`, which is a common case for swapping. 
    /// * `reexport_new_symbols_as_old`: if `true`, all public symbols the new crate will be reexported
    ///    in the `new_namespace` with the same full name those symbols had from the old crate in the `old_namespace`. 
    ///    See the "Important Note" in the struct-level documentation for more details.
    /// 
    pub fn new(
        // TODO FIXME: change to `Option<&str>`
        old_crate_name: String,
        old_namespace: Arc<CrateNamespace>,
        new_crate_object_file: IntoCrateObjectFile,
        new_namespace: Option<Arc<CrateNamespace>>,
        reexport_new_symbols_as_old: bool,
    ) -> Result<SwapRequest, &'static str> {
        let mut old_namespace = old_namespace;
        
        // Check that the old crate is actually in the old namespace; 
        // it may be currently loaded into the old namespace, 
        // but if not, we look to see if its crate object file is there.
        // If the old crate name is empty, that means there is no old crate to replace. 
        if old_crate_name != "" {
            // Find the exact namespace that contains the old crate or its object file (it can only be in the given old_namespace or its recursive children).
            let real_old_namespace = if let Some((_ocn, _ocr, real_old_namespace)) = CrateNamespace::get_crate_starting_with(&old_namespace, &old_crate_name) {
                Arc::clone(real_old_namespace)
            } else if let Some((_ocf, real_old_namespace)) = CrateNamespace::get_crate_object_file_starting_with(&old_namespace, &old_crate_name) {
                Arc::clone(real_old_namespace)
            } else {
                //  return OldCrateNotFound(old_crate_name, old_namespace)?;
                return Err("cannot find old_crate_file in old_namespace (recursively searched)");
            };
            trace!("SwapRequest::new(): changing old namespace from {:?} to {:?}", old_namespace.name, real_old_namespace.name);
            old_namespace = real_old_namespace;
        }
        
        // If no new namespace was given, use the same namespace that the old crate was found in.
        let mut new_namespace = new_namespace.unwrap_or_else(|| {
            trace!("SwapRequest::new(): new namespace was None, using old namespace {:?}", old_namespace.name);
            Arc::clone(&old_namespace)
        });

        // Try to resolve the new crate argument into an actual file.
        let verified_new_crate_file = match new_crate_object_file {
            IntoCrateObjectFile::File(f) => f,
            IntoCrateObjectFile::AbsolutePath(path) => match Path::get_absolute(&path) {
                Some(FileOrDir::File(f)) => f,
                _ => return Err("Couldn't find new crate object file file at absolute path"),
            }
            IntoCrateObjectFile::Prefix(prefix) => {
                let (new_crate_file, real_new_namespace) = CrateNamespace::get_crate_object_file_starting_with(&new_namespace, &prefix)
                    // .ok_or_else(|| NewCrateNotFound(...))?
                    .ok_or("cannot find new crate object file from prefix in new namespace (recursively searched)")?;
                trace!("SwapRequest::new(): changing new namespace from {:?} to {:?}", new_namespace.name, real_new_namespace.name);
                new_namespace = Arc::clone(real_new_namespace);
                new_crate_file
            }
        };

        Ok(SwapRequest {
            old_crate_name,
            old_namespace: ByAddress(old_namespace),
            new_crate_object_file: ByAddress(verified_new_crate_file),
            new_namespace: ByAddress(new_namespace),
            reexport_new_symbols_as_old,
        })
    }
}

/// The possible errors that can occur when trying to create a valid `SwapRequest`. 
pub enum InvalidSwapRequest {
    OldCrateNotFound(String, Arc<CrateNamespace>),
    NewCrateNotFound(IntoCrateObjectFile, Arc<CrateNamespace>),
    NewCratePathNotAbsolute(Path),
}


/// A "symbol map" from a fully-qualified demangled symbol String  
/// to weak reference to a `LoadedSection`.
/// This is used for relocations, and for looking up function names.
pub type SymbolMap = Trie<BString, WeakSectionRef>;
pub type SymbolMapIter<'a> = qp_trie::Iter<'a, &'a BString, &'a WeakSectionRef>;


/// A state transfer function is an arbitrary function called when swapping crates. 
/// See [`swap_crates()`](fn.CrateNamespace.swap_crates.html).
pub type StateTransferFunction = fn(&CrateNamespace, &CrateNamespace) -> Result<(), &'static str>;


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
    pub fn insert_crate_object_file(&self, crate_object_file_name: &str, content: &[u8]) -> Result<FileRef, &'static str> {
        let (_crate_type, _prefix, objfilename) = CrateType::from_module_name(crate_object_file_name)?;
        let cfile = MemFile::new(String::from(objfilename), &self.0)?;
        cfile.lock().write(content, 0)?;
        Ok(cfile)
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
    pub name: String,

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
    pub crate_tree: Mutex<Trie<BString, StrongCrateRef>>,

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

    /// The set of crates that have been previously unloaded (e.g., swapped out) from this namespace. 
    /// These are kept in memory as a performance optimization, such that if 
    /// they are ever requested to be swapped in again, we can swap them back in 
    /// almost instantly by avoiding the  expensive procedure of re-loading them into memory.
    /// 
    /// The unloaded cached crates are stored in the form of a CrateNamespace itself,
    /// such that we can easily query the cache for crates and symbols by name.
    /// 
    /// This is soft state that can be removed at any time with no effect on correctness.
    unloaded_crate_cache: Mutex<HashMap<SwapRequestList, CrateNamespace>>,

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
            unloaded_crate_cache: Mutex::new(HashMap::new()),
            fuzzy_symbol_matching: false,
        }
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

    pub fn enable_fuzzy_symbol_matching(&mut self) {
        self.fuzzy_symbol_matching = true;
    }

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

    /// Loads the specified application crate into memory, allowing it to be invoked.
    /// 
    /// The argument `load_symbols_as_singleton` determines what is done with the newly-loaded crate:      
    /// * If true, this application is loaded as a system-wide singleton crate and added to this namespace, 
    ///   and its public symbols are added to this namespace's symbol map, allowing other applications to depend upon it.
    /// * If false, this application is loaded normally and not added to this namespace in any way,
    ///   allowing it to be loaded again in the future as a totally separate instance.
    /// 
    /// Returns a Result containing the new crate itself.
    pub fn load_crate_as_application(
        &self, 
        crate_object_file: &FileRef, 
        kernel_mmi_ref: &MmiRef, 
        load_symbols_as_singleton: bool,
        verbose_log: bool
    ) -> Result<StrongCrateRef, &'static str> {
        
        debug!("load_crate_as_application(): trying to load application crate at {:?}", crate_object_file.lock().get_absolute_path());
        // Don't use a backup namespace when loading applications, 
        // it must be able to find all symbols in only this namespace (&self) and its backing namespaces.
        let new_crate_ref = self.load_crate_internal(crate_object_file, None, kernel_mmi_ref, verbose_log)?;

        if load_symbols_as_singleton {
            // if this is a singleton application, we add its public symbols (except "main")
            let new_crate = new_crate_ref.lock_as_ref();
            self.add_symbols_filtered(new_crate.sections.values(), 
                |sec| sec.name != "main", 
                verbose_log
            );
            self.crate_tree.lock().insert_str(&new_crate.crate_name, CowArc::clone_shallow(&new_crate_ref));
        } else {
            let new_crate = new_crate_ref.lock_as_ref();
            info!("loaded new application crate: {}, num sections: {}", new_crate.crate_name, new_crate.sections.len());
        }
        Ok(new_crate_ref)
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
    ) -> Result<usize, &'static str> {

        #[cfg(not(loscd_eval))]
        debug!("load_crate: trying to load crate at {:?}", crate_object_file.lock().get_absolute_path());
        let new_crate_ref = self.load_crate_internal(crate_object_file, temp_backup_namespace, kernel_mmi_ref, verbose_log)?;
        
        let (new_crate_name, new_syms) = {
            let new_crate = new_crate_ref.lock_as_ref();
            let new_syms = self.add_symbols(new_crate.sections.values(), verbose_log);
            (new_crate.crate_name.clone(), new_syms)
        };
            
        #[cfg(not(loscd_eval))]
        info!("loaded new crate {:?}, {} new symbols.", new_crate_name, new_syms);
        self.crate_tree.lock().insert(new_crate_name.into(), new_crate_ref);
        Ok(new_syms)
    }


    /// The internal function that does the work for loading crates,
    /// but does not add the crate nor its symbols to this namespace. 
    /// See [`load_crate`](#method.load_crate) and [`load_crate_as_application`](#method.load_crate_as_application).
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

        // Second, do all of the section parsing and loading.
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
        CrateNamespace {
            name: self.name.clone(),
            dir: self.dir.clone(),
            recursive_namespace: self.recursive_namespace.clone(),
            crate_tree: Mutex::new(self.crate_tree.lock().clone()),
            symbol_map: Mutex::new(self.symbol_map.lock().clone()),
            unloaded_crate_cache: Mutex::new(HashMap::new()),
            fuzzy_symbol_matching: self.fuzzy_symbol_matching,
        }
    }


    /// Swaps in new crates that can optionally replace existing crates in this `CrateNamespace`.
    /// 
    /// See the documentation of the [`SwapRequest`](#struct.SwapRequest.html) struct for more details.
    /// 
    /// In general, the strategy for replacing an old crate `C` with a new crate `C2` consists of three steps:
    /// 1) Load the new replacement crate `C2` from its object file.
    /// 2) Set up new relocation entries that redirect all dependencies on the old crate `C` to the new crate `C2`.
    /// 3) Remove crate `C` and clean it up, e.g., removing its entries from the symbol map.
    ///    Save the removed crate (and its symbol subtrie) in a cache for later use to expedite future swapping operations.
    /// 
    /// The given `CrateNamespace` is used as the backup namespace for resolving unknown symbols,
    /// in adddition to any recursive namespaces on which this namespace depends.
    /// 
    /// Upon a successful return, this namespacewill have the new crates in place of the old ones,
    /// and the old crates will be completely removed from this namespace. 
    /// In this namespace there will be no remaining dependencies on the old crates, 
    /// although crates in other namespaces may still include and depend upon those old crates.
    /// 
    /// # Arguments
    /// * `swap_requests`: a list of several `SwapRequest`s, in order to allow swapping multiple crates all at once 
    ///   as a single "atomic" procedure, which prevents weird linking/relocation errors, 
    ///   such as a new crate linking against an old crate that already exists in this namespace
    ///   instead of linking against the new one that we want to replace that old crate with. 
    /// * `override_namespace_dir`: the directories of object files from which missing crates should be loaded.
    ///   If a crate cannot be found in this directory set, this namespace's directory set will be searched for the crate. 
    ///   If `None`, only this `CrateNamespace`'s directory set will be used to find missing crates to be loaded.
    /// * `state_transfer_functions`: the fully-qualified symbol names of the state transfer functions, 
    ///   arbitrary functions that are invoked after all of the new crates are loaded but before the old crates are unloaded, 
    ///   in order to allow transfer of states from old crates to new crates or proper setup of states for the new crates.
    ///   These function should exist in the new namespace (or its directory set) and should take the form of a 
    ///   [`StateTransferFunction`](../type.StateTransferFunction.html).
    ///   The given functions are invoked with the arguments `(current_namespace, new_namespace)`, 
    ///   in which the `current_namespace` is the one currently running that contains the old crates,
    ///   and the `new_namespace` contains only newly-loaded crates that are not yet being used.
    ///   Both namespaces may (and likely will) contain more crates than just the old and new crates specified in the swap request list.
    /// * `kernel_mmi_ref`: a reference to the kernel's `MemoryManagementInfo`.
    /// * `verbose_log`: enable verbose logging.
    /// 
    /// # Warning: Correctness not guaranteed
    /// This function currently makes no attempt to guarantee correct operation after a crate is swapped. 
    /// For example, if the new crate changes a function or data structure, there is no guarantee that 
    /// other crates will be swapped alongside that one. 
    /// It will most likely error out, but this responsibility is currently left to the caller.
    /// 
    /// # Crate swapping optimizations
    /// When one or more crates is swapped out, they are not fully unloaded, but rather saved in a cache
    /// in order to accelerate future swapping commands. 
    /// 
    pub fn swap_crates(
        this_namespace: &Arc<CrateNamespace>,
        swap_requests: SwapRequestList,
        override_namespace_dir: Option<NamespaceDir>,
        state_transfer_functions: Vec<String>,
        kernel_mmi_ref: &MmiRef,
        verbose_log: bool,
    ) -> Result<(), &'static str> {

        #[cfg(not(loscd_eval))]
        debug!("swap_crates()[0]: override dir: {:?}\n\tswap_requests: {:?}", 
            override_namespace_dir.as_ref().map(|d| d.lock().get_name()), 
            swap_requests
        );

        #[cfg(loscd_eval)]
        let hpet_ref = hpet::get_hpet();
        #[cfg(loscd_eval)]
        let hpet = hpet_ref.as_ref().ok_or("couldn't get HPET timer")?;
        #[cfg(loscd_eval)]
        let hpet_start_swap = hpet.get_counter();
        
        let (namespace_of_new_crates, is_optimized) = {
            #[cfg(not(loscd_eval))] {
                // First, before we perform any expensive crate loading, let's try an optimization
                // based on cached crates that were unloaded during a previous swap operation. 
                if let Some(cached_crates) = this_namespace.unloaded_crate_cache.lock().remove(&swap_requests) {
                    warn!("Using optimized swap routine to swap in cached crates: {:?}", cached_crates.crate_names(true));
                    (cached_crates, true)
                } else {
                    // If no optimization is possible (no cached crates exist for this swap request), 
                    // then create a new CrateNamespace and load all of the new crate modules into it from scratch.
                    let nn = CrateNamespace::new(
                        String::from("temp_swap"), // format!("temp_swap--{:?}", swap_requests), 
                        // use the optionally-provided directory of crates instead of the current namespace's directories.
                        override_namespace_dir.unwrap_or_else(|| this_namespace.dir.clone()),
                        None,
                    );
                    let crate_file_iter = swap_requests.iter().map(|swap_req| swap_req.new_crate_object_file.deref());
                    nn.load_crates(crate_file_iter, Some(this_namespace), kernel_mmi_ref, verbose_log)?;
                    (nn, false)
                }
            }
            #[cfg(loscd_eval)] {
                let nn = CrateNamespace::new(
                    String::from("temp_swap"), // format!("temp_swap--{:?}", swap_requests), 
                    // use the optionally-provided directory of crates instead of the current namespace's directories.
                    override_namespace_dirs.unwrap_or_else(|| this_namespace.dir.clone()),
                    None,
                );
                let crate_file_iter = swap_requests.iter().map(|swap_req| swap_req.new_crate_object_file.deref());
                nn.load_crates(crate_file_iter, Some(this_namespace), kernel_mmi_ref, verbose_log)?;
                (nn, false)
            }
        };
            
        #[cfg(loscd_eval)]
        let hpet_after_load_crates = hpet.get_counter();

        #[cfg(not(loscd_eval))]
        let mut future_swap_requests: SwapRequestList = SwapRequestList::with_capacity(swap_requests.len());
        #[cfg(not(loscd_eval))]
        let cached_crates: CrateNamespace = CrateNamespace::new(
            format!("cached_crates--{:?}", swap_requests), 
            this_namespace.dir.clone(),
            None
        );


        #[cfg(loscd_eval)]
        let mut hpet_total_symbol_finding = 0;
        #[cfg(loscd_eval)]
        let mut hpet_total_rewriting_relocations = 0;
        #[cfg(loscd_eval)]
        let mut hpet_total_fixing_dependencies = 0;
        #[cfg(loscd_eval)]
        let mut hpet_total_bss_transfer = 0;

        // The name of the new crate in each swap request. There is one entry per swap request.
        let mut new_crate_names: Vec<String> = Vec::with_capacity(swap_requests.len());

        // Now that we have loaded all of the new modules into the new namepsace in isolation,
        // we simply need to fix up all of the relocations `WeakDependents` for each of the existing sections
        // that depend on the old crate that we're replacing here,
        // such that they refer to the new_module instead of the old_crate.
        for req in &swap_requests {
            let SwapRequest { old_crate_name, old_namespace, new_crate_object_file, new_namespace: _new_ns, reexport_new_symbols_as_old } = req;
            if old_crate_name == "" {
                // just adding a new crate, no replacement needed
                new_crate_names.push(String::new());
                continue; 
            }
            let reexport_new_symbols_as_old = *reexport_new_symbols_as_old;
            
            let (old_crate_ref, _) = CrateNamespace::get_crate_and_namespace(old_namespace, &*old_crate_name).ok_or_else(|| {
                error!("BUG: swap_crates(): couldn't find old_crate {:?} in namespace {:?}. This should be guaranteed in SwapRequest::new()!", old_crate_name, old_namespace.name);
                "BUG: swap_crates(): couldn't find old crate in old namespace. This should be guaranteed in SwapRequest::new()!"
            })?;
            let old_crate = old_crate_ref.lock_as_mut().ok_or_else(|| {
                error!("Unimplemented: swap_crates(), old_crate: {:?}, doesn't yet support deep copying shared crates to get a new exclusive mutable instance", old_crate_ref);
                "Unimplemented: swap_crates() doesn't yet support deep copying shared crates to get a new exclusive mutable instance"
            })?;
            
            let new_crate_name = Path::new(new_crate_object_file.lock().get_name()).file_stem().to_string();
            let new_crate_ref = if is_optimized {
                debug!("swap_crates(): OPTIMIZED: looking for new crate {:?} in cache", new_crate_name);
                namespace_of_new_crates.get_crate(&new_crate_name)
                    .ok_or_else(|| "BUG: swap_crates(): Couldn't get new crate from optimized cache")?
            } else {
                #[cfg(not(loscd_eval))]
                debug!("looking for newly-loaded crate {:?} in temp namespace", new_crate_name);
                namespace_of_new_crates.get_crate(&new_crate_name)
                    .ok_or_else(|| "BUG: Couldn't get new crate that should've just been loaded into a new temporary namespace")?
            };
            new_crate_names.push(new_crate_name);

            // scope the lock on the `new_crate_ref`
            {
                let mut new_crate = new_crate_ref.lock_as_mut().ok_or_else(|| 
                    "BUG: swap_crates(): new_crate was unexpectedly shared in another namespace (couldn't get as exclusively mutable)...?"
                )?;
                
                // currently we're always clearing out the new crate's reexports because we recalculate them every time
                new_crate.reexported_symbols.clear();

                let old_crate_name_without_hash = String::from(old_crate.crate_name_without_hash());
                let new_crate_name_without_hash = String::from(new_crate.crate_name_without_hash());
                let crates_have_same_name = old_crate_name_without_hash == new_crate_name_without_hash;

                // We need to find all of the "weak dependents" (sections that depend on the sections in the old crate)
                // and replace them by rewriting their relocation entries to point to the corresponding new section in the new_crate.
                //
                // Note that we only need to iterate through sections from the old crate that are public/global,
                // i.e., those that were previously added to this namespace's symbol map,
                // because other crates could not possibly depend on non-public sections in the old crate.
                for old_sec_name in &old_crate.global_symbols {
                    debug!("swap_crates(): looking for old_sec_name: {:?}", old_sec_name);
                    let (old_sec_ref, old_sec_ns) = this_namespace.get_symbol_and_namespace(old_sec_name.as_str())
                        .and_then(|(weak_sec_ref, ns)| weak_sec_ref.upgrade().map(|sec| (sec, ns)))
                        .ok_or_else(|| {
                            error!("BUG: swap_crates(): couldn't get/upgrade old crate's section: {:?}", old_sec_name);
                            "BUG: swap_crates(): couldn't get/upgrade old crate's section"
                        })?;

                    #[cfg(not(loscd_eval))]
                    debug!("swap_crates(): old_sec_name: {:?}, old_sec: {:?}", old_sec_name, old_sec_ref);
                    let mut old_sec = old_sec_ref.lock();
                    let old_sec_name_without_hash = old_sec.name_without_hash();


                    // This closure finds the section in the `new_crate` that corresponds to the given `old_sec` from the `old_crate`.
                    // And, if enabled, it will reexport that new section under the same name as the `old_sec`.
                    // We put this procedure in a closure because it's relatively expensive, allowing us to run it only when necessary.
                    let find_corresponding_new_section = |new_crate_reexported_symbols: &mut BTreeSet<BString>| {
                        // Use the new namespace to find the new source_sec that old target_sec should point to.
                        // The new source_sec must have the same name as the old one (old_sec here),
                        // otherwise it wouldn't be a valid swap -- the target_sec's parent crate should have also been swapped.
                        // The new namespace should already have that symbol available (i.e., we shouldn't have to load it on demand);
                        // if not, the swapping action was never going to work and we shouldn't go through with it.

                        // Find the section in the new crate that matches (or "fuzzily" matches) the current section from the old crate.
                        let new_crate_source_sec = if crates_have_same_name {
                            namespace_of_new_crates.get_symbol(&old_sec.name).upgrade()
                                .or_else(|| namespace_of_new_crates.get_symbol_starting_with(old_sec_name_without_hash).upgrade())
                        } else {
                            // here, the crates *don't* have the same name
                            if let Some(s) = replace_containing_crate_name(old_sec_name_without_hash, &old_crate_name_without_hash, &new_crate_name_without_hash) {
                                namespace_of_new_crates.get_symbol(&s).upgrade()
                                    .or_else(|| namespace_of_new_crates.get_symbol_starting_with(&s).upgrade())
                            } else {
                                // same as the default case above (crates have same name)
                                namespace_of_new_crates.get_symbol(&old_sec.name).upgrade()
                                    .or_else(|| namespace_of_new_crates.get_symbol_starting_with(old_sec_name_without_hash).upgrade())
                            }
                        }.ok_or_else(|| {
                            error!("swap_crates(): couldn't find section in the new crate that corresponds to a match of the old section {:?}", old_sec.name);
                            "couldn't find section in the new crate that corresponds to a match of the old section"
                        })?;
                        #[cfg(not(loscd_eval))]
                        debug!("swap_crates(): found match for old source_sec {:?}, new source_sec: {:?}", &*old_sec, &*new_crate_source_sec);

                        if reexport_new_symbols_as_old && old_sec.global {
                            // reexport the new source section under the old sec's name, i.e., redirect the old mapping to the new source sec
                            let reexported_name = BString::from(old_sec.name.as_str());
                            new_crate_reexported_symbols.insert(reexported_name.clone());
                            let _old_val = old_sec_ns.symbol_map.lock().insert(reexported_name, Arc::downgrade(&new_crate_source_sec));
                            if _old_val.is_none() { 
                                warn!("swap_crates(): reexported new crate section that replaces old section {:?}, but that old section unexpectedly didn't exist in the symbol map", old_sec.name);
                            }
                        }
                        Ok(new_crate_source_sec)
                    };


                    // the section from the `new_crate` that corresponds to the `old_sec_ref` from the `old_crate`
                    let mut new_sec_ref: Option<StrongSectionRef> = None;

                    // Iterate over all sections that depend on the old_sec. 
                    let mut dead_weak_deps_to_remove: Vec<usize> = Vec::new();
                    for (i, weak_dep) in old_sec.sections_dependent_on_me.iter().enumerate() {
                        let target_sec_ref = if let Some(sr) = weak_dep.section.upgrade() {
                            sr
                        } else {
                            trace!("Removing dead weak dependency from old_sec: {}", old_sec.name);
                            dead_weak_deps_to_remove.push(i);
                            continue;
                        };
                        let mut target_sec = target_sec_ref.lock();
                        let relocation_entry = weak_dep.relocation;

                        #[cfg(loscd_eval)]
                        let start_symbol_finding = hpet.get_counter();
                        

                        // get the section from the new crate that corresponds to the `old_sec`
                        let new_source_sec_ref = if let Some(ref nsr) = new_sec_ref {
                            trace!("using cached version of new source section");
                            nsr
                        } else {
                            trace!("Finding new source section from scratch");
                            let nsr = find_corresponding_new_section(&mut new_crate.reexported_symbols)?;
                            new_sec_ref.get_or_insert(nsr)
                        };

                        #[cfg(loscd_eval)] {
                            let end_symbol_finding = hpet.get_counter();
                            hpet_total_symbol_finding += (end_symbol_finding - start_symbol_finding);
                        }

                        let mut new_source_sec = new_source_sec_ref.lock();
                        #[cfg(not(loscd_eval))]
                        debug!("    swap_crates(): target_sec: {:?}, old source sec: {:?}, new source sec: {:?}", &*target_sec, &*old_sec, &*new_source_sec);

                        // If the target_sec's mapped pages aren't writable (which is common in the case of swapping),
                        // then we need to temporarily remap them as writable here so we can fix up the target_sec's new relocation entry.
                        {
                            #[cfg(loscd_eval)]
                            let start_rewriting_relocations = hpet.get_counter();

                            let mut target_sec_mapped_pages = target_sec.mapped_pages.lock();
                            let target_sec_initial_flags = target_sec_mapped_pages.flags();
                            if !target_sec_initial_flags.is_writable() {
                                target_sec_mapped_pages.remap(&mut kernel_mmi_ref.lock().page_table, target_sec_initial_flags | EntryFlags::WRITABLE)?;
                            }

                            write_relocation(
                                relocation_entry, 
                                &mut target_sec_mapped_pages, 
                                target_sec.mapped_pages_offset, 
                                new_source_sec.start_address(), 
                                verbose_log
                            )?;

                            #[cfg(loscd_eval)] {
                                let end_rewriting_relocations = hpet.get_counter();
                                hpet_total_rewriting_relocations += (end_rewriting_relocations - start_rewriting_relocations);
                            }

                            #[cfg(not(loscd_eval))] {
                                // If we temporarily remapped the target_sec's mapped pages as writable, undo that here
                                if !target_sec_initial_flags.is_writable() {
                                    target_sec_mapped_pages.remap(&mut kernel_mmi_ref.lock().page_table, target_sec_initial_flags)?;
                                };
                            }
                        }
                    

                        #[cfg(loscd_eval)]
                        let start_fixing_dependencies = hpet.get_counter();

                        // Tell the new source_sec that the existing target_sec depends on it.
                        // Note that we don't need to do this if we're re-swapping in a cached crate,
                        // because that crate's sections' dependents are already properly set up from when it was first swapped in.
                        if !is_optimized {
                            new_source_sec.sections_dependent_on_me.push(WeakDependent {
                                section: Arc::downgrade(&target_sec_ref),
                                relocation: relocation_entry,
                            });
                        }

                        // Tell the existing target_sec that it no longer depends on the old source section (old_sec_ref),
                        // and that it now depends on the new source_sec.
                        let mut found_strong_dependency = false;
                        for mut strong_dep in target_sec.sections_i_depend_on.iter_mut() {
                            if Arc::ptr_eq(&strong_dep.section, &old_sec_ref) && strong_dep.relocation == relocation_entry {
                                strong_dep.section = Arc::clone(&new_source_sec_ref);
                                found_strong_dependency = true;
                                break;
                            }
                        }
                        if !found_strong_dependency {
                            error!("Couldn't find/remove the existing StrongDependency from target_sec {:?} to old_sec {:?}",
                                target_sec.name, old_sec.name);
                            return Err("Couldn't find/remove the target_sec's StrongDependency on the old crate section");
                        }

                        #[cfg(loscd_eval)] {
                            let end_fixing_dependencies = hpet.get_counter();
                            hpet_total_fixing_dependencies += (end_fixing_dependencies - start_fixing_dependencies);
                        }
                    } // end of loop that iterates over all weak deps in the old_sec

                    for index in dead_weak_deps_to_remove {
                        old_sec.sections_dependent_on_me.remove(index);
                    }
                    
                } // end of loop that rewrites dependencies for sections that depend on the old_crate

                #[cfg(loscd_eval)]
                let hpet_start_bss_transfer = hpet.get_counter();

                // Go through all the BSS sections and copy over the old_sec into the new source_sec,
                // as they represent a static variable that would otherwise result in a loss of data.
                // Currently, AFAIK, static variables only exist in the form of .bss sections.
                for old_sec_ref in old_crate.bss_sections.values() {
                    let old_sec = old_sec_ref.lock();
                    let old_sec_name_without_hash = old_sec.name_without_hash();
                    // get the section from the new crate that corresponds to the `old_sec`
                    let new_dest_sec_ref = {
                        let mut iter = if crates_have_same_name {
                            new_crate.bss_sections.iter_prefix_str(old_sec_name_without_hash)
                        } else {
                            if let Some(s) = replace_containing_crate_name(old_sec_name_without_hash, &old_crate_name_without_hash, &new_crate_name_without_hash) {
                                new_crate.bss_sections.iter_prefix_str(&s)
                            } else {
                                new_crate.bss_sections.iter_prefix_str(old_sec_name_without_hash)
                            }
                        };
                        iter.next()
                            .filter(|_| iter.next().is_none()) // ensure single element
                            .map(|(_key, val)| val)
                    }.ok_or_else(|| 
                        "couldn't find destination section in new crate for copying old_sec's data into (BSS state transfer)"
                    )?;

                    // warn!("swap_crates(): copying BSS section from old {:?} to new {:?}", &*old_sec, new_dest_sec_ref);
                    old_sec.copy_section_data_to(&mut new_dest_sec_ref.lock())?;
                }

                #[cfg(loscd_eval)] {
                    let hpet_end_bss_transfer = hpet.get_counter();
                    hpet_total_bss_transfer += (hpet_end_bss_transfer - hpet_start_bss_transfer);
                }
            } // end of scope, drops lock on `new_crate_ref`
        } // end of iterating over all swap requests to fix up old crate dependents


        // apply the state transfer function, if provided
        for symbol in state_transfer_functions {
            let state_transfer_fn_sec_ref = namespace_of_new_crates.get_symbol_or_load(&symbol, Some(this_namespace), kernel_mmi_ref, verbose_log).upgrade()
                // as a backup, search fuzzily to accommodate state transfer function symbol names without full hashes
                .or_else(|| namespace_of_new_crates.get_symbol_starting_with(&symbol).upgrade())
                .ok_or("couldn't find specified state transfer function in the new CrateNamespace")?;
            
            let mut space: usize = 0;
            let st_fn = {
                let state_transfer_fn_sec = state_transfer_fn_sec_ref.lock();
                let mapped_pages = state_transfer_fn_sec.mapped_pages.lock();
                mapped_pages.as_func::<StateTransferFunction>(state_transfer_fn_sec.mapped_pages_offset, &mut space)?
            };
            info!("swap_crates(): invoking the state transfer function {:?}", symbol);
            st_fn(this_namespace, &namespace_of_new_crates)?;
        }

        // Remove all of the old crates now that we're fully done using them.
        // This doesn't mean each crate will be immediately dropped -- they still might be in use by other crates or tasks.
        for (req, new_crate_name) in swap_requests.iter().zip(new_crate_names.iter()) {
            let SwapRequest { old_crate_name, old_namespace, new_crate_object_file: _, new_namespace, reexport_new_symbols_as_old } = req;
            if old_crate_name == "" { continue; }
            // Remove the old crate from the namespace that it was previously in, and remove its sections' symbols too.
            if let Some(old_crate_ref) = old_namespace.crate_tree.lock().remove_str(old_crate_name) {
                {
                    let old_crate = old_crate_ref.lock_as_ref();
                    
                    #[cfg(not(loscd_eval))]
                    {
                        info!("  Removed old crate {:?} ({:?}) from namespace {}", old_crate_name, &*old_crate, old_namespace.name);

                        // Here, we setup the crate cache to enable the removed old crate to be quickly swapped back in in the future.
                        // This removed old crate will be useful when a future swap request includes the following:
                        // (1) the future `new_crate_object_file`        ==  the current `old_crate.object_file`
                        // (2) the future `old_crate_name`               ==  the current `new_crate_name`
                        // (3) the future `reexport_new_symbols_as_old`  ==  true if the old crate had any reexported symbols
                        //     -- to understand this, see the docs for `LoadedCrate.reexported_prefix`
                        let future_swap_req = SwapRequest {
                            old_crate_name: new_crate_name.clone(),
                            old_namespace: ByAddress(Arc::clone(new_namespace)),
                            new_crate_object_file: ByAddress(old_crate.object_file.clone()),
                            new_namespace: ByAddress(Arc::clone(old_namespace)),
                            reexport_new_symbols_as_old: !old_crate.reexported_symbols.is_empty(),
                        };
                        future_swap_requests.push(future_swap_req);
                    }
                    
                    // Remove all of the symbols belonging to the old crate from the namespace it was in.
                    // If reexport_new_symbols_as_old is true, we MUST NOT remove the old_crate's symbols from this symbol map,
                    // because we already replaced them above with mappings that redirect to the corresponding new crate sections.
                    if !reexport_new_symbols_as_old {
                        for symbol in &old_crate.global_symbols {
                            if old_namespace.symbol_map.lock().remove(symbol).is_none() {
                                error!("swap_crates(): couldn't find old symbol {:?} in the old crate's namespace: {}.", symbol, old_namespace.name);
                                return Err("couldn't find old symbol {:?} in the old crate's namespace");
                            }
                        }
                    }

                    // If the old crate had reexported its symbols, we should remove those reexports here,
                    // because they're no longer active since the old crate is being removed. 
                    for sym in &old_crate.reexported_symbols {
                        let _old_reexported_symbol = old_namespace.symbol_map.lock().remove(sym);
                        if _old_reexported_symbol.is_none() {
                            warn!("swap_crates(): the old_crate {:?}'s reexported symbol was not in its old namespace, couldn't be removed.", sym);
                        }
                    }

                    // TODO: could maybe optimize transfer of old symbols from this namespace to cached_crates namespace 
                    //       by saving the removed symbols above and directly adding them to the cached_crates.symbol_map instead of iterating over all old_crate.sections.
                    //       This wil only really be faster once qp_trie supports a non-iterator-based (non-extend) Trie merging function.
                    #[cfg(not(loscd_eval))]
                    cached_crates.add_symbols(old_crate.sections.values(), verbose_log); 
                } // drops lock for `old_crate_ref`
                
                #[cfg(not(loscd_eval))]
                cached_crates.crate_tree.lock().insert_str(old_crate_name, old_crate_ref);
                #[cfg(loscd_eval)]
                core::mem::forget(old_crate_ref);
            }
            else {
                error!("BUG: swap_crates(): failed to remove old crate {} from namespace {}", old_crate_name, old_namespace.name);
            }
        }

        #[cfg(loscd_eval)]
        let start_symbol_cleanup = hpet.get_counter();

        // Here, we move all of the new crates into the actual new namespace where they belong. 
        for (req, new_crate_name) in swap_requests.iter().zip(new_crate_names.iter()) {
            let new_crate_ref = namespace_of_new_crates.crate_tree.lock().remove_str(new_crate_name)
                .ok_or("BUG: swap_crates(): new crate specified by swap request was not found in the new namespace")?;
            
            #[cfg(not(loscd_eval))]
            debug!("swap_crates(): adding new crate {:?} to namespace {}", new_crate_ref, req.new_namespace.name);
            req.new_namespace.add_symbols(new_crate_ref.lock_as_ref().sections.values(), verbose_log);
            req.new_namespace.crate_tree.lock().insert_str(new_crate_name, new_crate_ref.clone());
        }
        
        // Other crates may have been loaded from their object files into the `namespace_of_new_crates` as dependendencies (required by the new crates specified by swap requests).
        // Thus, we need to move all **newly-loaded** crates from the `namespace_of_new_crates` into the proper new namespace;
        // for this, we add only the non-shared (exclusive) crates, because shared crates are those that were previously loaded (and came from the backup namespace).
        namespace_of_new_crates.for_each_crate(true, |new_crate_name, new_crate_ref| {
            if !new_crate_ref.is_shared() {
                // TODO FIXME: we may not want to add the new crate to this namespace, we might want to add it to one of its recursive namespaces
                //             (e.g., `this_namespace` could be an application ns, but the new_crate could be a kernel crate that belongs in `this_namespaces`'s recursive kernel namespace).
                //
                // To infer which actual namespace this newly-loaded crate should be added to (we don't want to add kernel crates to an application namespace),
                // we could iterate over all of the new crates in the swap requests that transitively depend on this new_crate,
                // and then look at which `new_namespace` those new crates in the swap requests were destined for.
                // Then, we use the highest-level namespace that we can, but it cannot be higher than any one `new_namespace'.
                // 
                // As an example, suppose there are 3 new crates from the swap request list that depend on this new_crate_ref here. 
                // If two of those were application crates that belong to a higher-level "applications" namespace (their `new_namespace`), 
                // but one of them was a kernel crate that belonged to a lower "kernel" namespace that was found in that "applications" namespace recursive children, 
                // the highest-level namespace we could add this `new_crate_ref` to would be that "kernel" namespace,
                // meaning that we had inferred that this `new_crate_ref` was also a kernel crate that belonged in the "kernel" namespace.
                //
                // This follows the rule that crates in a lower-level namespace cannot depend on those in a higher-level namespace. 
                //
                // Note that we don't just want to put the crate in the lowest namespace we can, 
                // because that could result in putting an application crate in a kernel namespace. 
                // 
                let target_ns = this_namespace;

                #[cfg(not(loscd_eval))]
                warn!("swap_crates(): untested scenario of adding new non-requested (depedency) crate {:?} to namespace {}", new_crate_ref, target_ns.name);
                target_ns.add_symbols(new_crate_ref.lock_as_ref().sections.values(), verbose_log);
                target_ns.crate_tree.lock().insert_str(new_crate_name, new_crate_ref.clone());
            }
            else {
                #[cfg(not(loscd_eval))] {
                    if this_namespace.get_crate(new_crate_name).is_some() { 
                        debug!("shared crate {} was in the current namespace like we expected.", new_crate_name);
                    }
                    else {
                        error!("BUG: shared crate {} was not already in the current (backup) namespace", new_crate_name);
                    }
                }
            }
            true
        });

        #[cfg(loscd_eval)]
        let end_symbol_cleanup = hpet.get_counter();


        // TODO: rewrite this closure to put the new crate object files into the proper namespace folder (not just all files in the override).
        //
        // Now that we've fixed up the already-loaded (running) crates, 
        // we need to move the new crate object files from new namespace directory set to the old (this_namespace),
        // such that future usage of those crates will load the appropriate new crate object files. 
        let copy_directory_contents = |source_dir_ref: &DirRef, dest_dir_ref: &DirRef| -> Result<(), &'static str> {
            if Arc::ptr_eq(source_dir_ref, dest_dir_ref) { 
                return Ok(()); // no action required if the directories are the same (not overridden)
            }
            let mut source_dir = source_dir_ref.lock();
            let mut dest_dir   = dest_dir_ref.lock();
            for fs_node_name in &source_dir.list() {
                let file_node = match source_dir.get(fs_node_name) {
                    Some(FileOrDir::File(f)) => FileOrDir::File(f),
                    _ => {
                        warn!("Skipping unexpected directory {} in source namespace directory", fs_node_name);
                        continue;
                    }
                };
                if let Some(mut node) = source_dir.remove(&file_node) {
                    node.set_parent_dir(Arc::downgrade(dest_dir_ref));
                    let _old_node = dest_dir.insert(node)?;
                    use fs_node::FsNode;
                    debug!("swap_crates(): replaced old crate object file {:?}", _old_node.map(|n| n.get_name()));
                }
            }
            Ok(())
        };
        copy_directory_contents(&namespace_of_new_crates.dir, &this_namespace.dir)?; // TODO: FIXME: see above TODO, this is wrong

        #[cfg(not(loscd_eval))]
        {
            debug!("swap_crates() [end]: adding old_crates to cache. \n   future_swap_requests: {:?}, \n   old_crates: {:?}", 
                future_swap_requests, cached_crates.crate_names(true));
            this_namespace.unloaded_crate_cache.lock().insert(future_swap_requests, cached_crates);
        }


        #[cfg(loscd_eval)] {
            // done with everything, print out values
            warn!("
                load crates, {}
                find symbols, {}
                rewrite relocations, {}
                fix dependencies, {}
                BSS transfer, {}
                symbol cleanup, {}
                HPET PERIOD (femtosec): {}
                ",
                hpet_after_load_crates - hpet_start_swap,
                hpet_total_symbol_finding,
                hpet_total_rewriting_relocations,
                hpet_total_fixing_dependencies,
                hpet_total_bss_transfer,
                end_symbol_cleanup - start_symbol_cleanup,
                hpet.counter_period_femtoseconds(),
            );
        }

        Ok(())
        // here, "namespace_of_new_crates is dropped, but its crates have already been added to the current namespace 
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

        let old_sec = old_section.lock();

        for weak_dep in &old_sec.sections_dependent_on_me {
            let target_sec_ref = weak_dep.section.upgrade().ok_or_else(|| "couldn't upgrade WeakDependent.section")?;
            let mut target_sec = target_sec_ref.lock();
            let relocation_entry = weak_dep.relocation;

            let mut new_source_sec = new_section.lock();
            debug!("rewrite_section_dependents(): target_sec: {:?}, old source sec: {:?}, new source sec: {:?}", target_sec, old_sec, new_source_sec);

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
                    new_source_sec.start_address(), 
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
                new_source_sec.sections_dependent_on_me.push(WeakDependent {
                    section: Arc::downgrade(&target_sec_ref),
                    relocation: relocation_entry,
                });
            // }

            // Tell the existing target_sec that it no longer depends on the old source section (old_sec_ref),
            // and that it now depends on the new source_sec.
            let mut found_strong_dependency = false;
            for mut strong_dep in target_sec.sections_i_depend_on.iter_mut() {
                if Arc::ptr_eq(&strong_dep.section, old_section) && strong_dep.relocation == relocation_entry {
                    strong_dep.section = Arc::clone(new_section);
                    found_strong_dependency = true;
                    break;
                }
            }
            if !found_strong_dependency {
                error!("Couldn't find/remove the existing StrongDependency from target_sec {:?} to old_sec {:?}",
                    target_sec.name, old_sec.name);
                return Err("Couldn't find/remove the target_sec's StrongDependency on the old crate section");
            }
        }

        Ok(())
    }


    /// The primary internal routine for parsing and loading all of the sections.
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
        let crate_name    = abs_path.file_stem().to_string();

        // First, check to make sure this crate hasn't already been loaded. 
        // Regular, non-singleton application crates aren't added to the CrateNamespace, so they can be multiply loaded.
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
        debug!("Parsing Elf kernel crate: {:?}, size {:#x}({})", crate_name, size_in_bytes, size_in_bytes);

        // allocate enough space to load the sections
        let section_pages = allocate_section_pages(&elf_file, kernel_mmi_ref)?;
        let text_pages   = section_pages.executable_pages.map(|(tp, range)| (Arc::new(Mutex::new(tp)), range));
        let rodata_pages = section_pages.read_only_pages.map( |(rp, range)| (Arc::new(Mutex::new(rp)), range));
        let data_pages   = section_pages.read_write_pages.map(|(dp, range)| (Arc::new(Mutex::new(dp)), range));

        // Check the symbol table to get the set of sections that are global (publicly visible).
        let global_sections: BTreeSet<usize> = {
            // For us to properly load the ELF file, it must NOT have been fully stripped,
            // meaning that it must still have its symbol table section. Otherwise, relocations will not work.
            let symtab = find_symbol_table(&elf_file)?;

            let mut globals: BTreeSet<usize> = BTreeSet::new();
            use xmas_elf::symbol_table::Entry;
            for entry in symtab.iter() {
                // Include all symbols with "GLOBAL" binding, regardless of visibility.  
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
            object_file:             crate_object_file, 
            sections:                BTreeMap::new(),
            text_pages:              text_pages.clone(),
            rodata_pages:            rodata_pages.clone(),
            data_pages:              data_pages.clone(),
            global_symbols:          BTreeSet::new(),
            bss_sections:            Trie::new(),
            reexported_symbols:      BTreeSet::new(),
        });
        let new_crate_weak_ref = CowArc::downgrade(&new_crate);
        
        // this maps section header index (shndx) to LoadedSection
        let mut loaded_sections: BTreeMap<usize, StrongSectionRef> = BTreeMap::new(); 
        // the list of all symbols in this crate that are public (global) 
        let mut global_symbols: BTreeSet<BString> = BTreeSet::new();
        // the map of BSS section names to the actual BSS section
        let mut bss_sections: Trie<BString, StrongSectionRef> = Trie::new();

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

            let write: bool = sec.flags() & SHF_WRITE     == SHF_WRITE;
            let exec:  bool = sec.flags() & SHF_EXECINSTR == SHF_EXECINSTR;

            // First, check for executable sections, which can only be .text sections.
            if exec && !write {
                if let Some(name) = sec_name.get(TEXT_PREFIX.len() ..) {
                    let demangled = demangle(name).to_string();

                    // We already copied the content of all .text sections above, 
                    // so here we just record the metadata into a new `LoadedSection` object.
                    if let Some((ref tp_ref, ref tp_range)) = text_pages {
                        let is_global = global_sections.contains(&shndx);
                        if is_global {
                            global_symbols.insert(demangled.clone().into());
                        }

                        let text_offset = sec.offset() as usize;
                        let dest_vaddr = tp_range.start + text_offset;

                        loaded_sections.insert(shndx, 
                            Arc::new(Mutex::new(LoadedSection::new(
                                SectionType::Text,
                                demangled,
                                Arc::clone(tp_ref),
                                text_offset,
                                dest_vaddr,
                                sec_size,
                                is_global,
                                new_crate_weak_ref.clone(),
                            )))
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

            // Second, if not executable, handle writable .data sections
            else if write && sec_name.starts_with(DATA_PREFIX) {
                if let Some(name) = sec_name.get(DATA_PREFIX.len() ..) {
                    let name = if name.starts_with(RELRO_PREFIX) {
                        let relro_name = name.get(RELRO_PREFIX.len() ..).ok_or("Couldn't get name of .data.rel.ro. section")?;
                        relro_name
                    }
                    else {
                        name
                    };
                    let demangled = demangle(name).to_string();
                    
                    if let Some((ref dp_ref, ref mut dp)) = read_write_pages_locked {
                        // here: we're ready to copy the data/bss section to the proper address
                        let dest_vaddr = dp.address_at_offset(data_offset)
                            .ok_or_else(|| "BUG: data_offset wasn't within data_pages")?;
                        let dest_slice: &mut [u8]  = dp.as_slice_mut(data_offset, sec_size)?;
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

                        let is_global = global_sections.contains(&shndx);
                        if is_global {
                            global_symbols.insert(demangled.clone().into());
                        }
                        
                        loaded_sections.insert(shndx, 
                            Arc::new(Mutex::new(LoadedSection::new(
                                SectionType::Data,
                                demangled,
                                Arc::clone(dp_ref),
                                data_offset,
                                dest_vaddr,
                                sec_size,
                                is_global,
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

            // Third, if not executable and not a writable .data section, handle writable .bss sections
            else if write && sec_name.starts_with(BSS_PREFIX) {
                if let Some(name) = sec_name.get(BSS_PREFIX.len() ..) {
                    let demangled = demangle(name).to_string();
                    
                    // we still use DataSection to represent the .bss sections, since they have the same flags
                    if let Some((ref dp_ref, ref mut dp)) = read_write_pages_locked {
                        // here: we're ready to fill the bss section with zeroes at the proper address
                        let dest_vaddr = dp.address_at_offset(data_offset)
                            .ok_or_else(|| "BUG: data_offset wasn't within data_pages")?;
                        let dest_slice: &mut [u8]  = dp.as_slice_mut(data_offset, sec_size)?;
                        for b in dest_slice {
                            *b = 0;
                        };

                        let is_global = global_sections.contains(&shndx);
                        if is_global {
                            global_symbols.insert(demangled.clone().into());
                        }
                        
                        let sec_ref = Arc::new(Mutex::new(LoadedSection::new(
                            SectionType::Bss,
                            demangled.clone(),
                            Arc::clone(dp_ref),
                            data_offset,
                            dest_vaddr,
                            sec_size,
                            is_global,
                            new_crate_weak_ref.clone(),
                        )));
                        loaded_sections.insert(shndx, sec_ref.clone());
                        bss_sections.insert(demangled.into(), sec_ref);

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

            // Fourth, if neither executable nor writable, handle .rodata sections
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

                        let is_global = global_sections.contains(&shndx);
                        if is_global {
                            global_symbols.insert(demangled.clone().into());
                        }
                        
                        loaded_sections.insert(shndx, 
                            Arc::new(Mutex::new(LoadedSection::new(
                                SectionType::Rodata,
                                demangled,
                                Arc::clone(rp_ref),
                                rodata_offset,
                                dest_vaddr,
                                sec_size,
                                is_global,
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

            // Fifth, if neither executable nor writable nor .rodata, handle the `.gcc_except_table` section
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

                    // .gcc_except_table section is not globally visible
                    let is_global = false;
                    
                    loaded_sections.insert(shndx, 
                        Arc::new(Mutex::new(LoadedSection::new(
                            SectionType::GccExceptTable,
                            sec_name.to_string(),
                            Arc::clone(rp_ref),
                            rodata_offset,
                            dest_vaddr,
                            sec_size,
                            is_global,
                            new_crate_weak_ref.clone(),
                        )))
                    );

                    rodata_offset += round_up_power_of_two(sec_size, sec_align);
                }
                else {
                    return Err("no rodata_pages were allocated when handling .gcc_except_table");
                }
            }

            // Sixth, if neither executable nor writable nor .rodata nor .gcc_except_table, handle the `.eh_frame` section
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

                    // .gcc_except_table section is not globally visible
                    let is_global = false;
                    
                    loaded_sections.insert(shndx, 
                        Arc::new(Mutex::new(LoadedSection::new(
                            SectionType::EhFrame,
                            sec_name.to_string(),
                            Arc::clone(rp_ref),
                            rodata_offset,
                            dest_vaddr,
                            sec_size,
                            is_global,
                            new_crate_weak_ref.clone(),
                        )))
                    );

                    rodata_offset += round_up_power_of_two(sec_size, sec_align);
                }
                else {
                    return Err("no rodata_pages were allocated when handling .eh_frame");
                }
            }

            // Finally, any other section type is considered unhandled, so return an error!
            else {
                // currently not using sections like ".debug_gdb_scripts"
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
            new_crate_mut.global_symbols = global_symbols;
            new_crate_mut.bss_sections = bss_sections;
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
        let new_crate = new_crate_ref.lock_as_ref();
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

            // currently not using debug sections
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

                    let source_and_target_are_same_section = Arc::ptr_eq(&source_sec_ref, &target_sec_ref);
                    let relocation_entry = RelocationEntry::from_elf_relocation(rela_entry);

                    write_relocation(
                        relocation_entry,
                        &mut target_sec_mapped_pages,
                        target_sec.mapped_pages_offset,
                        if source_and_target_are_same_section { target_sec.start_address() } else { source_sec_ref.lock().start_address() },
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
        if let Some(ref tp) = new_crate.text_pages { 
            tp.0.lock().remap(&mut kernel_mmi_ref.lock().page_table, TEXT_SECTION_FLAGS)?;
        }
        if let Some(ref rp) = new_crate.rodata_pages {
            rp.0.lock().remap(&mut kernel_mmi_ref.lock().page_table, RODATA_SECTION_FLAGS)?;
        }
        // data/bss sections are already mapped properly, since they're supposed to be writable

        Ok(())
    }

    
    /// Adds the given symbol to this namespace's symbol map.
    /// If the symbol already exists in the symbol map, this replaces the existing symbol with the new one, warning if they differ in size.
    /// Returns true if the symbol was added, and false if it already existed and thus was merely replaced.
    fn add_symbol(
        existing_symbol_map: &mut SymbolMap,
        new_section_key: String,
        new_section_ref: &StrongSectionRef,
        log_replacements: bool,
    ) -> bool {
        match existing_symbol_map.entry(new_section_key.into()) {
            qp_trie::Entry::Occupied(mut old_val) => {
                if log_replacements {
                    if let Some(old_sec_ref) = old_val.get().upgrade() {
                        if !Arc::ptr_eq(&old_sec_ref, new_section_ref) {
                            // debug!("       add_symbol(): replacing section: old: {:?}, new: {:?}", old_sec_ref, new_section_ref);
                            let old_sec = old_sec_ref.lock();
                            let new_sec = new_section_ref.lock();
                            if new_sec.size() != old_sec.size() {
                                warn!("         add_symbol(): Unexpectedly replacing differently-sized section: old: ({}B) {:?}, new: ({}B) {:?}", old_sec.size(), old_sec.name, new_sec.size(), new_sec.name);
                            } 
                            else {
                                warn!("         add_symbol(): Replacing new symbol already present: old {:?}, new: {:?}", old_sec.name, new_sec.name);
                            }
                        }
                    }
                }
                old_val.insert(Arc::downgrade(new_section_ref));
                false
            }
            qp_trie::Entry::Vacant(new_entry) => {
                if log_replacements { 
                    debug!("         add_symbol(): Adding brand new symbol: new: {:?}", new_section_ref);
                }
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
        log_replacements: bool,
    ) -> usize
        where I: IntoIterator<Item = &'a StrongSectionRef>,
              F: Fn(&LoadedSection) -> bool
    {
        let mut existing_map = self.symbol_map.lock();

        // add all the global symbols to the symbol map, in a way that lets us inspect/log each one
        let mut count = 0;
        for sec_ref in sections.into_iter() {
            let (sec_name, condition) = {
                let sec = sec_ref.lock();
                (
                    sec.name.clone(),
                    filter_func(&sec) && sec.global
                )
            };
            
            if condition {
                // trace!("add_symbols_filtered(): adding symbol {:?}", sec_ref);
                let added = CrateNamespace::add_symbol(&mut existing_map, sec_name, sec_ref, log_replacements);
                if added {
                    count += 1;
                }
            }
            // else {
            //     trace!("add_symbols_filtered(): skipping symbol {:?}", sec_ref);
            // }
        }
        
        count
    }

    
    /// Finds the crate that contains the given `VirtualAddress` in its loaded code,
    /// also searching any recursive namespaces as well.
    /// 
    /// By default, only executable sections (`.text`) are searched, since typically the only use case 
    /// for this function is to search for an instruction pointer (program counter) address.
    /// However, if `search_all_section_types` is `true`, both the read-only and read-write sections
    /// will be included in the search, e.g., `.rodata`, `.data`, `.bss`. 
    /// 
    /// If a `starting_crate` is provided, the search will begin from that crate
    /// and include all of its dependencies recursively. 
    /// If no `starting_crate` is provided, all crates in this namespace will be searched, in no particular order.
    /// 
    /// TODO FIXME: currently starting_crate is searched non-recursively, meaning that its dependencies are excluded.
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
    /// it will iterate through **every** loaded crate.
    pub fn get_crate_containing_address(
        &self, 
        virt_addr: VirtualAddress, 
        starting_crate: Option<&StrongCrateRef>,
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
        
        // If a starting crate was given, search that first. 
        if let Some(crate_ref) = starting_crate {
            if crate_contains_vaddr(crate_ref) {
                return Some(crate_ref.clone());
            }
        }

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


    /// Finds the section that contains the given `VirtualAddress` in its loaded code,
    /// also searching any recursive namespaces as well.
    /// 
    /// By default, only executable sections (`.text`) are searched, since typically the only use case 
    /// for this function is to search for an instruction pointer (program counter) address.
    /// However, if `search_all_section_types` is `true`, both the read-only and read-write sections
    /// will be included in the search, e.g., `.rodata`, `.data`, `.bss`. 
    /// 
    /// If a `starting_crate` is provided, the search will begin from that crate
    /// and include all of its dependencies recursively. 
    /// If no `starting_crate` is provided, all crates in this namespace will be searched, in no particular order.
    /// 
    /// TODO FIXME: currently starting_crate is searched non-recursively, meaning that its dependencies are excluded.
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
    /// it will iterate through **every** section in **every** loaded crate,
    /// not just the publicly-visible (global) ones. 
    pub fn get_section_containing_address(
        &self, 
        virt_addr: VirtualAddress, 
        starting_crate: Option<&StrongCrateRef>,
        search_all_section_types: bool,
    ) -> Option<(StrongSectionRef, usize)> {

        // First, we find the crate that contains the address, then later we narrow it down.
        let containing_crate = self.get_crate_containing_address(virt_addr, starting_crate, search_all_section_types)?;
        let crate_locked = containing_crate.lock_as_ref();

        // Second, we find the section in that crate that contains the address.
        for sec_ref in crate_locked.sections.values() {
            // trace!("get_section_containing_address: locking sec_ref: {:?}", sec_ref);
            let sec = sec_ref.lock();
            // .text sections are always included, other sections are included if requested.
            let eligible_section = sec.typ == SectionType::Text || search_all_section_types;
            
            // If the section's address bounds contain the address, then we've found it.
            // Only a single section can contain the address, so it's safe to stop once we've found a match.
            if eligible_section && sec.address_range.contains(&virt_addr) {
                let offset = virt_addr.value() - sec.start_address().value();
                return Some((sec_ref.clone(), offset));
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
        let mut fuzzy_matched_symbol_name: Option<String> = None;

        let (weak_sec, _found_in_ns) = if !fuzzy_matching {
            // use exact (non-fuzzy) matching
            temp_backup_namespace.get_symbol_and_namespace(demangled_full_symbol)?
        } else {
            // use fuzzy matching (ignoring the symbol hash suffix)
            let fuzzy_matches = temp_backup_namespace.find_symbols_starting_with_and_namespace(LoadedSection::section_name_without_hash(demangled_full_symbol));
            match fuzzy_matches.as_slice() {
                [(sec_name, weak_sec, _found_in_ns)] => {
                    fuzzy_matched_symbol_name = Some(sec_name.clone());
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
            sec.lock().parent_crate.upgrade().or_else(|| {
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
        info!("Symbol {:?} not initially found, using {} symbol {} from crate {:?} in backup namespace {:?} in new namespace {:?}",
            demangled_full_symbol, 
            if fuzzy_matching { "fuzzy-matched" } else { "" },
            fuzzy_matched_symbol_name.unwrap_or_default(),
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
            if self.get_crate(potential_crate_file_path.file_stem()).is_some() {
                trace!("  (skipping already-loaded crate {:?})", potential_crate_file_path);
                continue;
            }
            #[cfg(not(loscd_eval))]
            info!("Symbol {:?} not initially found in namespace {:?}, attempting to load crate {:?} into namespace {:?} that may contain it.", 
                demangled_full_symbol, self.name, potential_crate_name, ns_of_crate_file.name);

            match ns_of_crate_file.load_crate(&potential_crate_file, temp_backup_namespace, kernel_mmi_ref, verbose_log) {
                Ok(_num_new_syms) => {
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
            // Skip non-allocated sections; they don't need to be loaded into memory
            if sec.flags() & SHF_ALLOC == 0 {
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
            let write: bool = sec.flags() & SHF_WRITE     == SHF_WRITE;
            let exec:  bool = sec.flags() & SHF_EXECINSTR == SHF_EXECINSTR;
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
    let mut frame_allocator = FRAME_ALLOCATOR.try().ok_or("couldn't get FRAME_ALLOCATOR")?.lock();
    let allocated_pages = allocate_pages_by_bytes(size_in_bytes).ok_or("Couldn't allocate_pages_by_bytes, out of virtual address space")?;
    kernel_mmi_ref.lock().page_table.map_allocated_pages(allocated_pages, flags | EntryFlags::PRESENT | EntryFlags::WRITABLE, frame_allocator.deref_mut())
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
    // https://docs.rs/goblin/0.0.24/goblin/elf/reloc/index.html
    match relocation_entry.typ {
        R_X86_64_32 => {
            let target_ref: &mut u32 = target_sec_mapped_pages.as_type_mut(target_offset)?;
            let source_val = source_sec_vaddr.value().wrapping_add(relocation_entry.addend);
            if verbose_log { trace!("                    target_ptr: {:#X}, source_val: {:#X} (from sec_vaddr {:#X})", target_ref as *mut _ as usize, source_val, source_sec_vaddr); }
            *target_ref = source_val as u32;
        }
        R_X86_64_64 => {
            let target_ref: &mut u64 = target_sec_mapped_pages.as_type_mut(target_offset)?;
            let source_val = source_sec_vaddr.value().wrapping_add(relocation_entry.addend);
            if verbose_log { trace!("                    target_ptr: {:#X}, source_val: {:#X} (from sec_vaddr {:#X})", target_ref as *mut _ as usize, source_val, source_sec_vaddr); }
            *target_ref = source_val as u64;
        }
        R_X86_64_PC32 |
        R_X86_64_PLT32 => {
            let target_ref: &mut u32 = target_sec_mapped_pages.as_type_mut(target_offset)?;
            let source_val = source_sec_vaddr.value().wrapping_add(relocation_entry.addend).wrapping_sub(target_ref as *mut _ as usize);
            if verbose_log { trace!("                    target_ptr: {:#X}, source_val: {:#X} (from sec_vaddr {:#X})", target_ref as *mut _ as usize, source_val, source_sec_vaddr); }
            *target_ref = source_val as u32;
        }
        R_X86_64_PC64 => {
            let target_ref: &mut u64 = target_sec_mapped_pages.as_type_mut(target_offset)?;
            let source_val = source_sec_vaddr.value().wrapping_add(relocation_entry.addend).wrapping_sub(target_ref as *mut _ as usize);
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
