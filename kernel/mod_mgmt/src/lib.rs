#![allow(clippy::blocks_in_if_conditions)]
#![no_std]
#![feature(int_roundings)]
#![feature(let_chains)]

#[macro_use] extern crate alloc;
#[macro_use] extern crate log;

use core::{cmp::max, fmt, mem::size_of, ops::{Deref, Range}};
use alloc::{
    boxed::Box, 
    collections::{BTreeMap, btree_map, BTreeSet}, 
    string::{String, ToString}, 
    sync::{Arc, Weak}, vec::Vec
};
use spin::{Mutex, Once};
use xmas_elf::{ElfFile, sections::{SHF_ALLOC, SHF_EXECINSTR, SHF_TLS, SHF_WRITE, SectionData, ShType}, symbol_table::{Binding, Type}};
use memory::{MmiRef, MemoryManagementInfo, VirtualAddress, MappedPages, PteFlags, allocate_pages_by_bytes, allocate_frames_by_bytes_at};
use bootloader_modules::BootloaderModule;
use cow_arc::CowArc;
use rustc_demangle::demangle;
use qp_trie::Trie;
use fs_node::{FileOrDir, File, FileRef, DirRef};
use vfs_node::VFSDirectory;
use path::Path;
use memfs::MemFile;
use hashbrown::HashMap;
use rangemap::RangeMap;
pub use crate_name_utils::*;
pub use crate_metadata::*;

pub mod parse_nano_core;
pub mod replace_nano_core_crates;
mod serde;

/// The name of the directory that contains all of the CrateNamespace files.
pub const NAMESPACES_DIRECTORY_NAME: &str = "namespaces";

/// The name of the directory that contains all other "extra_files" contents.
pub const EXTRA_FILES_DIRECTORY_NAME: &str = "extra_files";
const EXTRA_FILES_DIRECTORY_DELIMITER: char = '!';

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

/// The thread-local storage (TLS) area "image" that is used as the initial data for each `Task`.
/// When spawning a new task, the new task will create its own local TLS area
/// with this `TlsInitializer` as the initial data values.
/// 
/// # Implementation Notes/Shortcomings
/// Currently, a single system-wide `TlsInitializer` instance is shared across all namespaces.
/// In the future, each namespace should hold its own TLS sections in its TlsInitializer area.
/// 
/// However, this is quite complex because each namespace must be aware of the TLS sections
/// in BOTH its underlying recursive namespace AND its (multiple) "parent" namespace(s)
/// that recursively depend on it, since no two TLS sections can conflict (have the same offset).
/// 
/// Thus, we stick with a singleton `TlsInitializer` instance, which makes sense 
/// because it behaves much like an allocator, in that it reserves space (index ranges) in the TLS area.
static TLS_INITIALIZER: Mutex<TlsInitializer> = Mutex::new(TlsInitializer::empty());


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
/// If a file does not have an expected crate prefix according to [`CrateType::from_module_name()`],
/// then it is treated as part of "extra_files"; see [`parse_extra_file()`] for more.
/// 
/// Returns a tuple of: 
/// * the top-level root "namespaces" directory that contains all other namespace directories,
/// * the directory of the default kernel crate namespace.
fn parse_bootloader_modules_into_files(
    bootloader_modules: Vec<BootloaderModule>,
    kernel_mmi: &mut MemoryManagementInfo
) -> Result<(DirRef, NamespaceDir), &'static str> {

    // create the top-level directory to hold all default namespaces
    let namespaces_dir = VFSDirectory::create(NAMESPACES_DIRECTORY_NAME.to_string(), root::get_root())?;
    // create the top-level directory to hold all extra files
    let extra_files_dir = VFSDirectory::create(EXTRA_FILES_DIRECTORY_NAME.to_string(), root::get_root())?;

    // a map that associates a prefix string (e.g., "sse" in "ksse#crate.o") to a namespace directory of object files 
    let mut prefix_map: BTreeMap<String, NamespaceDir> = BTreeMap::new();

    // Closure to create the directory for a new namespace.
    let create_dir = |dir_name: &str| -> Result<NamespaceDir, &'static str> {
        VFSDirectory::create(dir_name.to_string(), &namespaces_dir).map(|d| NamespaceDir(d))
    };

    let mut process_module = |name: &str, size, pages| -> Result<_, &'static str> {
        let (crate_type, prefix, file_name) = if let Ok((c, p, f)) = CrateType::from_module_name(name) {
            (c, p, f)
        } else {
            parse_extra_file(name, size, pages, Arc::clone(&extra_files_dir))?;
            return Ok(());
        };

        let dir_name = format!("{}{}", prefix, crate_type.default_namespace_name());
        // debug!("Module: {:?}, size {}, mp: {:?}", name, size, pages);

        let create_file = |dir: &DirRef| {
            MemFile::from_mapped_pages(pages, file_name.to_string(), size, dir)
        };
        // Get the existing (or create a new) namespace directory corresponding to the given directory name.
        let _new_file = match prefix_map.entry(dir_name.clone()) {
            btree_map::Entry::Vacant(vacant) => create_file( vacant.insert(create_dir(&dir_name)?) )?,
            btree_map::Entry::Occupied(occ)  => create_file( occ.get() )?,
        };
        Ok(())
    };

    for m in bootloader_modules {
        let frames = allocate_frames_by_bytes_at(m.start_address(), m.size_in_bytes())
            .map_err(|_e| "Failed to allocate frames for bootloader module")?;
        let pages = allocate_pages_by_bytes(m.size_in_bytes())
            .ok_or("Couldn't allocate virtual pages for bootloader module")?;
        let mp = kernel_mmi.page_table.map_allocated_pages_to(
            pages,
            frames,
            // we never need to write to bootloader-provided modules
            PteFlags::new().valid(true),
        )?;

        let name = m.name();
        let size = m.size_in_bytes();

        if name == "modules.cpio.lz4" {
            // The bootloader modules were compressed/archived into one large module at build time,
            // so we must extract them here.

            #[cfg(feature = "extract_boot_modules")]
            {
                let bytes = mp.as_slice(0, size)?;
                let tar = lz4_flex::block::decompress_size_prepended(bytes)
                    .map_err(|_e| "lz4 decompression of bootloader modules failed")?;
                /*
                 * TODO: avoid using tons of heap space for decompression by
                 *       allocating a separate MappedPages instance and using `decompress_into()`.
                 *       We can determined the uncompressed size ahead of time using the following:
                 */
                let _uncompressed_size = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
                for entry in cpio_reader::iter_files(&tar) {
                    let name = entry.name();
                    let bytes = entry.file();
                    let size = bytes.len();
                    let mut mp = {
                        let flags = PteFlags::new().valid(true).writable(true);
                        let allocated_pages = allocate_pages_by_bytes(size).ok_or("couldn't allocate pages")?;
                        kernel_mmi.page_table.map_allocated_pages(allocated_pages, flags)?
                    };
                    {
                        let slice = mp.as_slice_mut(0, size)?;
                        slice.copy_from_slice(bytes);
                    }
                    process_module(name, size, mp)?;
                }
                continue;
            }
            #[cfg(not(feature = "extract_boot_modules"))]
            {
                let err_msg = "BUG: found `modules.cpio.lz4` bootloader module, but the `extract_boot_modules` feature was disabled!";
                error!("{}", err_msg);
                return Err(err_msg);
            }
        }

        process_module(name, size, mp)?;
    }

    debug!("Created namespace directories: {:?}", prefix_map.keys().map(|s| &**s).collect::<Vec<&str>>().join(", "));
    Ok((
        namespaces_dir,
        prefix_map.remove(CrateType::Kernel.default_namespace_name()).ok_or("BUG: no default namespace found")?,
    ))
}

/// Adds the given extra file to the directory of extra files
/// 
/// See the top-level Makefile target "extra_files" for an explanation of how these work.
/// Basically, they are arbitrary files that are included by the bootloader as modules
/// (files that exist as areas of pre-loaded memory).
/// 
/// Their file paths are encoded by flattening directory hierarchies into a the file name,
/// using `'!'` (exclamation marks) to replace the directory delimiter `'/'`.
/// 
/// Thus, for example, a file named `"foo!bar!me!test.txt"` will be placed at the path
/// `/extra_files/foo/bar/me/test.txt`.
fn parse_extra_file(
    extra_file_name: &str,
    extra_file_size: usize,
    extra_file_mp: MappedPages,
    extra_files_dir: DirRef
) -> Result<FileRef, &'static str> {

    let mut file_name = extra_file_name;
    
    let mut parent_dir = extra_files_dir;
    let mut iter = extra_file_name.split(EXTRA_FILES_DIRECTORY_DELIMITER).peekable();
    while let Some(path_component) = iter.next() {
        if iter.peek().is_some() {
            let existing_dir = parent_dir.lock().get_dir(path_component);
            parent_dir = existing_dir
                .or_else(|| VFSDirectory::create(path_component.to_string(), &parent_dir).ok())
                .ok_or_else(|| {
                    error!("Failed to get or create directory {:?} for extra file {:?}", path_component, extra_file_name);
                    "Failed to get or create directory for extra file"
                })?;
        } else {
            file_name = path_component;
            break;
        }
    }

    MemFile::from_mapped_pages(
        extra_file_mp,
        file_name.to_string(),
        extra_file_size,
        &parent_dir
    )
}



/// A "symbol map" from a fully-qualified demangled symbol String  
/// to weak reference to a `LoadedSection`.
/// This is used for relocations, and for looking up function names.
pub type SymbolMap = Trie<StrRef, WeakSectionRef>;


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
        let cfile = MemFile::create(String::from(objfilename), &self.0)?;
        cfile.lock().write_at(content, 0)?;
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
                .unwrap_or_else(|| "<Locked>".to_string())
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
        if let Some(_removed_app_crate) = self.namespace.crate_tree().lock().remove(&crate_locked.crate_name) {
            // Second, remove all of the crate's global symbols from the namespace's symbol map.
            let mut symbol_map = self.namespace.symbol_map().lock();
            for sec_to_remove in crate_locked.global_sections_iter() {
                match symbol_map.remove(&sec_to_remove.name) {
                    Some(_removed) => {
                        // trace!("Removed symbol {}: {:?}", sec_to_remove.name, _removed.upgrade());
                    }
                    None => {
                        error!("NOTE: couldn't find old symbol {:?} in the old crate {:?} to remove from namespace {:?}.", sec_to_remove.name, crate_locked.crate_name, self.namespace.name());
                    }
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
    /// stored as a map in which the crate's string name
    /// is the key that maps to the value, a strong reference to a crate.
    /// It is a strong reference because a crate must not be removed
    /// as long as it is part of any namespace,
    /// and a single crate can be part of multiple namespaces at once.
    /// For example, the "core" (Rust core library) crate is essentially
    /// part of every single namespace, simply because most other crates rely upon it. 
    crate_tree: Mutex<Trie<StrRef, StrongCrateRef>>,

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

    /// The thread-local storage (TLS) area "image" that is used as the initial data for each `Task`
    /// that is spawned and runs within this `CrateNamespace`.
    /// When spawning a new task, the new task will create its own local TLS area
    /// with this `tls_initializer` as the local data.
    /// 
    /// NOTE: this is currently a global system-wide singleton. See the static [`static@TLS_INITIALIZER`] for more.
    tls_initializer: &'static Mutex<TlsInitializer>,

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
            tls_initializer: &TLS_INITIALIZER,
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

    /// Returns a new copy of this namespace's initial TLS area,
    /// which can be used as the initial TLS area data for a new task.
    pub fn get_tls_initializer_data(&self) -> TlsDataImage {
        self.tls_initializer.lock().get_data()
    }

    #[doc(hidden)]
    pub fn crate_tree(&self) -> &Mutex<Trie<StrRef, StrongCrateRef>> {
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
    /// This is a slow method mostly for debugging, since it allocates a new vector of crate names.
    pub fn crate_names(&self, recursive: bool) -> Vec<StrRef> {
        let mut crates: Vec<StrRef> = self.crate_tree.lock().keys().cloned().collect();

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
    /// [`CowArc::clone()`] function on the returned value.
    pub fn get_crate(&self, crate_name: &str) -> Option<StrongCrateRef> {
        self.crate_tree.lock().get(crate_name.as_bytes())
            .map(CowArc::clone_shallow)
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
    /// [`CowArc::clone()`] function on the returned value.
    pub fn get_crate_and_namespace<'n>(
        namespace: &'n Arc<CrateNamespace>,
        crate_name: &str
    ) -> Option<(StrongCrateRef, &'n Arc<CrateNamespace>)> {
        namespace.crate_tree.lock().get(crate_name.as_bytes())
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
    ) -> Vec<(StrRef, StrongCrateRef, &'n Arc<CrateNamespace>)> { 
        // First, we make a list of matching crates in this namespace. 
        let crates = namespace.crate_tree.lock();
        let mut crates_in_this_namespace = crates.iter_prefix(crate_name_prefix.as_bytes())
            .map(|(key, val)| (key.clone(), val.clone_shallow(), namespace))
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
    ) -> Option<(StrRef, StrongCrateRef, &'n Arc<CrateNamespace>)> { 
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
    pub fn method_get_crate_object_files_starting_with(
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
    pub fn method_get_crate_object_file_starting_with(
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
            namespace.crate_tree.lock().insert(new_crate.crate_name.clone(), CowArc::clone_shallow(&new_crate_ref));
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
        self.crate_tree.lock().insert(new_crate_name, new_crate_ref.clone_shallow());
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
            tls_initializer: &TLS_INITIALIZER,
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
            let target_sec = weak_dep.section.upgrade().ok_or("couldn't upgrade WeakDependent.section")?;
            let relocation_entry = weak_dep.relocation;

            debug!("rewrite_section_dependents(): target_sec: {:?}, old_sec: {:?}, new_sec: {:?}", target_sec, old_section, new_section);

            // If the target_sec's mapped pages aren't writable (which is common in the case of swapping),
            // then we need to temporarily remap them as writable here so we can fix up the target_sec's new relocation entry.
            {
                let mut target_sec_mapped_pages = target_sec.mapped_pages.lock();
                let target_sec_initial_flags = target_sec_mapped_pages.flags();
                if !target_sec_initial_flags.is_writable() {
                    target_sec_mapped_pages.remap(&mut kernel_mmi_ref.lock().page_table, target_sec_initial_flags.writable(true))?;
                }

                write_relocation(
                    relocation_entry,
                    target_sec_mapped_pages.as_slice_mut(0, target_sec.mapped_pages_offset + target_sec.size)?,
                    target_sec.mapped_pages_offset,
                    new_section.virt_addr,
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
        let size_in_bytes = crate_file.len();
        let abs_path      = Path::new(crate_file.get_absolute_path());
        let crate_name    = StrRef::from(crate_name_from_path(&abs_path));

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

        // Check that elf_file is a relocatable type 
        use xmas_elf::header::Type;
        let typ = elf_file.header.pt2.type_().as_type();
        if typ != Type::Relocatable {
            error!("load_crate_sections(): crate \"{}\" was a {:?} Elf File, must be Relocatable!", &crate_name, typ);
            return Err("not a relocatable elf file");
        }

        // If a `.theseus_merged` section exists, then the object file's sections have been merged by a partial relinking step.
        // If so, then we can use a much faster version of loading/linking.
        const THESEUS_MERGED_SEC_NAME: &str = ".theseus_merged";
        const THESEUS_MERGED_SEC_SHNDX: u16 = 1;
        let sections_are_merged = elf_file.section_header(THESEUS_MERGED_SEC_SHNDX)
            .map(|sec| sec.get_name(&elf_file) == Ok(THESEUS_MERGED_SEC_NAME))
            .unwrap_or(false);

        // Allocate enough space to load the sections
        let section_pages = allocate_section_pages(&elf_file, kernel_mmi_ref)?;
        let text_pages   = section_pages.executable_pages.map(|(tp, range)| (Arc::new(Mutex::new(tp)), range));
        let rodata_pages = section_pages.read_only_pages.map( |(rp, range)| (Arc::new(Mutex::new(rp)), range));
        let data_pages   = section_pages.read_write_pages.map(|(dp, range)| (Arc::new(Mutex::new(dp)), range));

        // Create the new `LoadedCrate` now such that its sections can refer back to it.
        let new_crate = CowArc::new(LoadedCrate {
            crate_name,
            debug_symbols_file:      Arc::downgrade(&crate_object_file),
            object_file:             crate_object_file, 
            sections:                HashMap::new(),
            text_pages:              text_pages.clone(),
            rodata_pages:            rodata_pages.clone(),
            data_pages:              data_pages.clone(),
            global_sections:         BTreeSet::new(),
            tls_sections:            BTreeSet::new(),
            data_sections:           BTreeSet::new(),
            reexported_symbols:      BTreeSet::new(),
        });
        let new_crate_weak_ref = CowArc::downgrade(&new_crate);

        let load_sections_fn = if sections_are_merged { 
            Self::load_crate_with_merged_sections
        } else {
            Self::load_crate_with_separate_sections
        };

        let SectionMetadata {
            loaded_sections,
            global_sections,
            tls_sections,
            data_sections,
         } = load_sections_fn(
            self,
            &elf_file,
            new_crate_weak_ref,
            text_pages,
            rodata_pages,
            data_pages,
        )?;

        // Set up the new_crate's sections, since we couldn't do it when `new_crate` was created.
        {
            let mut new_crate_mut = new_crate.lock_as_mut()
                .ok_or("BUG: load_crate_sections(): couldn't get exclusive mutable access to new_crate")?;
            new_crate_mut.sections        = loaded_sections;
            new_crate_mut.global_sections = global_sections;
            new_crate_mut.tls_sections    = tls_sections;
            new_crate_mut.data_sections   = data_sections;
        }

        Ok((new_crate, elf_file))
    }


    /// An internal routine to load and populate the sections of a crate's object file
    /// if those sections have already been merged.
    /// 
    /// This is the "new, acclerated" way to load sections, and is used by `load_crate_sections()`
    /// for object files that **have** been modified by Theseus's special partial relinking script.
    /// 
    /// This works by iterating over all symbols in the object file 
    /// and creating section entries for each one of those symbols only.
    /// The actual section data can be loaded quickly because they have been merged into top-level sections,
    /// e.g., .text, .rodata, etc, instead of being kept as individual function or data sections.
    fn load_crate_with_merged_sections(
        &self,
        elf_file:     &ElfFile,
        new_crate:    WeakCrateRef,
        text_pages:   Option<(Arc<Mutex<MappedPages>>, Range<VirtualAddress>)>,
        rodata_pages: Option<(Arc<Mutex<MappedPages>>, Range<VirtualAddress>)>,
        data_pages:   Option<(Arc<Mutex<MappedPages>>, Range<VirtualAddress>)>,
    ) -> Result<SectionMetadata, &'static str> {
        
        let mut text_pages_locked       = text_pages  .as_ref().map(|(tp, tp_range)| (tp.clone(), tp.lock(), tp_range.start));
        let mut read_only_pages_locked  = rodata_pages.as_ref().map(|(rp, rp_range)| (rp.clone(), rp.lock(), rp_range.start));
        let mut read_write_pages_locked = data_pages  .as_ref().map(|(dp, dp_range)| (dp.clone(), dp.lock(), dp_range.start));

        // The section header offset of the first read-only section, which is, in order of existence:
        // .rodata, .eh_frame, .gcc_except_table, .tdata
        let mut read_only_offset: Option<usize> = None;
        // The section header offset of the first read-write section, which is .data or .bss
        let mut read_write_offset:   Option<usize> = None;
 
        // We need to track various section `shndx`s to differentiate between 
        // the different types of "OBJECT" symbols and "TLS" symbols.
        //
        // For example, we use the `.rodata` shndx in order to determine whether
        // an "OBJECT" symbol is read-only (.rodata) or read-write (.data or .bss).
        //
        // Note: we *could* just get the section header for each symtab entry, 
        //       but that is MUCH slower than tracking them here.
        let mut rodata_shndx:            Option<Shndx>                     = None;
        let mut data_shndx:              Option<Shndx>                     = None;
        let mut bss_shndx_and_offset:    Option<(Shndx, usize)>            = None;
        let mut tdata_shndx_and_section: Option<(Shndx, StrongSectionRef)> = None;
        let mut tbss_shndx_and_section:  Option<(Shndx, StrongSectionRef)> = None;

        // The set of `LoadedSections` that will be parsed and populated into this `new_crate`.
        let mut loaded_sections: HashMap<usize, StrongSectionRef> = HashMap::new(); 
        let mut data_sections:   BTreeSet<usize> = BTreeSet::new();
        let mut tls_sections:    BTreeSet<usize> = BTreeSet::new();
        let mut last_shndx = 0;

        // Iterate over all "allocated" sections to copy their data from the object file into the above `MappedPages`s.
        // This includes .text, .rodata, .data, .bss, .gcc_except_table, .eh_frame, etc.
        //
        // We currently perform eager copying because MappedPages doesn't yet support backing files or demand paging.
        for (shndx, sec) in elf_file.section_iter().enumerate() {
            let sec_flags = sec.flags();
            // Skip non-allocated sections, because they don't appear in the loaded object file.
            if sec_flags & SHF_ALLOC == 0 {
                continue; 
            }

            // get the relevant section info, i.e., size, alignment, and data contents
            let sec_size   = sec.size()   as usize;
            let sec_offset = sec.offset() as usize;
            let is_write   = sec_flags & SHF_WRITE     == SHF_WRITE;
            let is_exec    = sec_flags & SHF_EXECINSTR == SHF_EXECINSTR;
            let is_tls     = sec_flags & SHF_TLS       == SHF_TLS;

            let mut is_rodata = false;
            let mut is_eh_frame = false;
            let mut is_gcc_except_table = false;

            // Declare the items needed to populate/create a new `LoadedSection`.
            let typ: SectionType;
            let mapped_pages: &mut MappedPages;
            let mapped_pages_ref: &Arc<Mutex<MappedPages>>;
            let mapped_pages_offset: usize;
            let virt_addr: VirtualAddress;

            // If executable, copy the .text section data into `text_pages`.
            if is_exec {
                typ = SectionType::Text;
                // There is only one text section, so no offset is needed
                mapped_pages_offset = 0;
                (mapped_pages_ref, mapped_pages, virt_addr) = text_pages_locked.as_mut()
                    .map(|(tp_ref, tp, tp_start_vaddr)| (tp_ref, tp, *tp_start_vaddr + mapped_pages_offset))
                    .ok_or("BUG: ELF file contained a .text section, but no text_pages were allocated")?;
            }

            // Otherwise, if writable (excluding TLS), copy the .data/.bss section into `data_pages`.
            else if is_write && !is_tls {
                match sec.get_type() {
                    Ok(ShType::ProgBits) => {
                        typ = SectionType::Data;
                        data_shndx.get_or_insert(shndx);
                    }
                    Ok(ShType::NoBits) => {
                        typ = SectionType::Bss;
                        bss_shndx_and_offset.get_or_insert((shndx, sec_offset));
                    }
                    _other => {
                        error!("BUG: writable section was neither PROGBITS (.data) nor NOBITS (.bss): type: {:?}, {:X?}", _other, sec);
                        return Err("BUG: writable section was neither PROGBITS (.data) nor NOBITS (.bss)");
                    }
                };

                let starting_offset_of_data = read_write_offset.get_or_insert(sec_offset);
                mapped_pages_offset = sec_offset - *starting_offset_of_data;
                (mapped_pages_ref, mapped_pages, virt_addr) = read_write_pages_locked.as_mut()
                    .map(|(dp_ref, dp, dp_start_vaddr)| (dp_ref, dp, *dp_start_vaddr + mapped_pages_offset))
                    .ok_or("BUG: ELF file contained a .data/.bss section, but no data_pages were allocated")?;
                data_sections.insert(shndx);
            }

            // Otherwise, if TLS section, copy its data into `rodata_pages`.
            // Although TLS sections have "WAT" flags (write, alloc, TLS),
            // we load TLS sections into the same read-only pages as other read-only sections
            // because they contain thread-local storage initializer data that is only read from.
            else if is_tls {
                match sec.get_type() {
                    Ok(ShType::ProgBits) => {
                        typ = SectionType::TlsData;
                        let read_only_start = read_only_offset.get_or_insert(sec_offset);
                        mapped_pages_offset = sec_offset - *read_only_start;
                    }
                    Ok(ShType::NoBits) => {
                        typ = SectionType::TlsBss;
                        // Here: a TLS .tbss section has no actual content, so we use a max-value offset
                        // as a canary value to ensure it cannot be used to index into a MappedPages.
                        mapped_pages_offset = usize::MAX;
                    }
                    _other => {
                        error!("BUG: TLS section was neither PROGBITS (.tdata) nor NOBITS (.tbss): type: {:?}, {:X?}", _other, sec);
                        return Err("BUG: TLS section was neither PROGBITS (.tdata) nor NOBITS (.tbss)");
                    }
                };

                (mapped_pages_ref, mapped_pages) = read_only_pages_locked.as_mut()
                    .map(|(rp_ref, rp, _)| (rp_ref, rp))
                    .ok_or("BUG: ELF file contained a .tdata/.tbss section, but no rodata_pages were allocated")?;
                // Use a placeholder vaddr; it will be replaced in `add_new_dynamic_tls_section()` below.
                virt_addr = VirtualAddress::zero(); 
                tls_sections.insert(shndx);
            }

            // Otherwise, if .rodata, .eh_frame, or .gcc_except_table, copy its data into `rodata_pages`.
            else if {
                match sec.get_name(elf_file) {
                    Ok(RODATA_SECTION_NAME)           => is_rodata           = true,
                    Ok(EH_FRAME_SECTION_NAME)         => is_eh_frame         = true,
                    Ok(GCC_EXCEPT_TABLE_SECTION_NAME) => is_gcc_except_table = true,
                    Ok(_other)                        => { /* fall through to next `else if` block */ }
                    Err(_e)                           => {
                        error!("BUG: Error: {:?}, couldn't get section name for {:?}", _e, sec);
                        return Err("BUG: couldn't get section name");
                    }
                }
                is_rodata || is_eh_frame || is_gcc_except_table
            } {
                if is_rodata {
                    typ = SectionType::Rodata;
                    rodata_shndx = Some(shndx);
                } else if is_eh_frame {
                    typ = SectionType::EhFrame;
                } else if is_gcc_except_table {
                    typ = SectionType::GccExceptTable;
                } else {
                    unreachable!()
                }

                let read_only_start = read_only_offset.get_or_insert(sec_offset);
                mapped_pages_offset = sec_offset - *read_only_start;
                (mapped_pages_ref, mapped_pages, virt_addr) = read_only_pages_locked.as_mut()
                    .map(|(rp_ref, rp, rp_start_vaddr)| (rp_ref, rp, *rp_start_vaddr + mapped_pages_offset))
                    .ok_or("BUG: ELF file contained a read-only section, but no rodata_pages were allocated")?;
            }

            // Finally, any other section type is considered unhandled, so return an error!
            else {
                // .debug_* sections are handled separately and loaded on demand later.
                let sec_name = sec.get_name(elf_file);
                if sec_name.map_or(false, |n| n.starts_with(".debug")) {
                    continue;
                }
                error!("unhandled sec, name: {:?}, {:X?}", sec_name, sec);
                return Err("load_crate_with_merged_sections(): section with unhandled type, name, or flags!");
            }

            // Actually copy the section data from the ELF file to the given destination MappedPages.
            // Skip TLS BSS (.tbss) sections, which have no data and occupy no space in memory.
            if typ != SectionType::TlsBss {
                let dest_slice: &mut [u8] = mapped_pages.as_slice_mut(mapped_pages_offset, sec_size)?;
                match sec.get_data(elf_file) {
                    Ok(SectionData::Undefined(sec_data)) => dest_slice.copy_from_slice(sec_data),
                    Ok(SectionData::Empty) => dest_slice.fill(0),
                    _other => {
                        error!("Couldn't get section data for merged section: {:?}", _other);
                        return Err("couldn't get section data for merged section");
                    }
                }
            }

            // Create a new `LoadedSection` to represent this section.
            let new_section = LoadedSection::new(
                typ,
                section_name_str_ref(&typ),
                Arc::clone(mapped_pages_ref),
                mapped_pages_offset,
                virt_addr,
                sec_size,
                false, // no merged sections are global
                new_crate.clone(),
            );

            let new_section_ref = if is_tls {
                // Add the new TLS section to this namespace's initial TLS area,
                // which will reserve/obtain a new offset into that TLS area which holds this section's data.
                // This will also update the section's virtual address field to hold that offset value,
                // which is used for relocation entries that ask for a section's offset from the TLS base.
                let (_tls_offset, new_tls_section) = self.tls_initializer.lock()
                    .add_new_dynamic_tls_section(new_section, sec.align() as usize)
                    .map_err(|_| "Failed to add new dynamic TLS section")?;

                // trace!("Updated new TLS section to have offset {:#X}: {:?}", _tls_offset, new_tls_section);
                if new_tls_section.typ == SectionType::TlsData {
                    tdata_shndx_and_section = Some((shndx, Arc::clone(&new_tls_section)));
                } else {
                    tbss_shndx_and_section = Some((shndx, Arc::clone(&new_tls_section)));
                }
                new_tls_section
            } else {
                Arc::new(new_section)
            };

            loaded_sections.insert(shndx, new_section_ref);
            last_shndx = shndx + 1;
        }

        // Now that we've copied all the section data from the object file to the various mapped pages,
        // we can populate the crate's sets of global sections by iterating over the symbol table.
        // The above loop just handled the merged sections, none of which should be made global.
        let mut global_sections: BTreeSet<usize> = BTreeSet::new();

        let symtab = find_symbol_table(elf_file)?;
        use xmas_elf::symbol_table::Entry;
        for (_sym_num, symbol_entry) in symtab.iter().enumerate() {
            let sec_type = symbol_entry.get_type().map_err(|_e| {
                error!("BUG: Error: {:?}, couldn't get symtab entry type: {}", _e, symbol_entry as &dyn Entry);
                "BUG: couldn't get symtab entry type"
            })?;
            // Skip irrelevant symbols, e.g., NOTYPE, SECTION, etc.
            if let Type::NoType | Type::Section = sec_type {
                continue;
            }
            // trace!("Symtab entry {:?}\n\tnum: {}, value: {:#X}, size: {}, type: {:?}, bind: {:?}, vis: {:?}, shndx: {}", 
            //     symbol_entry.get_name(&elf_file).unwrap(), _sym_num, symbol_entry.value(), symbol_entry.size(), symbol_entry.get_type().unwrap(), symbol_entry.get_binding().unwrap(), symbol_entry.get_other(), symbol_entry.shndx()
            // );

            // Get the relevant section info from the symtab entry
            let sec_size = symbol_entry.size() as usize;
            let sec_value = symbol_entry.value() as usize;
            let sec_name = symbol_entry.get_name(elf_file).map_err(|_e| {
                error!("BUG: Error: {:?}, couldn't get symtab entry name: {}", _e, symbol_entry as &dyn Entry);
                "BUG: couldn't get symtab entry name"
            })?;
            let sec_binding = symbol_entry.get_binding().map_err(|_e| {
                error!("BUG: Error: {:?}, couldn't get symtab entry binding: {}", _e, symbol_entry as &dyn Entry);
                "BUG: couldn't get symtab entry binding"
            })?;
            let is_global = sec_binding == Binding::Global;
            let is_tls = sec_type == Type::Tls;
            let demangled = demangle(sec_name).to_string().as_str().into();

            // Declare the items we need to create a new `LoadedSection`.
            let typ: SectionType;
            let mapped_pages: &Arc<Mutex<MappedPages>>;
            let mapped_pages_offset: usize;
            let virt_addr: VirtualAddress;

            // Handle a "FUNC" symbol, which exists in .text
            if sec_type == Type::Func {
                let (tp_ref, tp_start_vaddr) = text_pages_locked.as_ref()
                    .map(|(mp_arc, _, mp_vaddr)| (mp_arc, *mp_vaddr))
                    .ok_or("BUG: found FUNC symbol but no text_pages were allocated")?;

                typ = SectionType::Text;
                mapped_pages = tp_ref;
                // no additional offset below, because .text is always the first (and only) exec section.
                mapped_pages_offset = sec_value;
                virt_addr = tp_start_vaddr + mapped_pages_offset;
            }

            // Handle an "OBJECT" symbol, which exists in .rodata, .data, or .bss
            else if sec_type == Type::Object {
                let sym_shndx = symbol_entry.shndx() as Shndx;
                // Handle .rodata symbol
                if Some(sym_shndx) == rodata_shndx {
                    let (rp_ref, rp_start_vaddr) = read_only_pages_locked.as_ref()
                        .map(|(mp_arc, _, mp_vaddr)| (mp_arc, *mp_vaddr))
                        .ok_or("BUG: found OBJECT symbol in .rodata but no rodata_pages were allocated")?;

                    typ = SectionType::Rodata;
                    mapped_pages = rp_ref;
                    // no additional offset below, because .rodata is always the first read-only section.
                    mapped_pages_offset = sec_value; 
                    virt_addr = rp_start_vaddr + mapped_pages_offset;
                } 
                // Handle .data/.bss symbol
                else {
                    data_sections.insert(last_shndx);

                    let (dp_ref, dp_start_vaddr) = read_write_pages_locked.as_ref()
                        .map(|(mp_arc, _, mp_vaddr)| (mp_arc, *mp_vaddr))
                        .ok_or("BUG: found OBJECT symbol in .data/.bss but no data_pages were allocated")?;
                    let read_write_start = read_write_offset.ok_or("BUG: found OBJECT symbol in .data/.bss but `data_offset` was unknown")?;

                    if Some(sym_shndx) == data_shndx {
                        typ = SectionType::Data;
                        // no additional offset below, because .data is always the first read-write section.
                        mapped_pages_offset = sec_value;
                    } else if let Some((bss_shndx, bss_offset)) = bss_shndx_and_offset && sym_shndx == bss_shndx {
                        typ = SectionType::Bss;
                        mapped_pages_offset = sec_value + (bss_offset - read_write_start);
                    } else {
                        error!("BUG: found OBJECT symbol with an shndx that wasn't in .rodata, .data, or .bss: {}", symbol_entry as &dyn Entry);
                        return Err("BUG: found OBJECT symbol with an shndx that wasn't in .rodata, .data, or .bss");
                    };
                    mapped_pages = dp_ref;
                    virt_addr = dp_start_vaddr + mapped_pages_offset;
                }
            }

            // Handle a "TLS" symbol, which exists in .tdata or .tbss
            else if is_tls {
                let sym_shndx = symbol_entry.shndx() as Shndx;
                // A TLS symbol with an shndx of 0 is a reference to a foreign dependency,
                // so we skip it just like we do for `NoType` symbols at the top of this loop.
                if sym_shndx == 0 {
                    continue;
                }

                // TLS sections have been copied into the read-only pages.
                // The merged TLS sections have already been dynamically assigned a virtual address above,
                // so we can calculate a TLS symbol's vaddr and mapped_pages_offset by adding 
                // the symbol's value (`sec_value`) to that of the corresponding merged section.
                let rp_ref = read_only_pages_locked.as_ref()
                    .map(|(mp_arc, ..)| mp_arc)
                    .ok_or("BUG: found TLS symbol but no rodata_pages were allocated")?;

                if let Some((tdata_shndx, ref tdata_sec)) = tdata_shndx_and_section && sym_shndx == tdata_shndx {
                    typ = SectionType::TlsData;
                    mapped_pages_offset = tdata_sec.mapped_pages_offset + sec_value;
                    virt_addr = tdata_sec.virt_addr + sec_value;
                } else if let Some((tbss_shndx, ref tbss_sec)) = tbss_shndx_and_section && sym_shndx == tbss_shndx {
                    typ = SectionType::TlsBss;
                    // Here: a TLS .tbss section has no actual content, so we use a max-value offset
                    // as a canary value to ensure it cannot be used to index into a MappedPages.
                    mapped_pages_offset = usize::MAX;
                    virt_addr = tbss_sec.virt_addr + sec_value;
                } else {
                    error!("BUG: found TLS symbol with an shndx that wasn't in .tdata or .tbss: {}", symbol_entry as &dyn Entry);
                    return Err("BUG: found TLS symbol with an shndx that wasn't in .tdata or .tbss");
                };
                mapped_pages = rp_ref;
            }

            else {
                error!("Found unexpected symbol type: {}", symbol_entry as &dyn Entry);
                return Err("Found unexpected symbol type");
            }

            // Create the new `LoadedSection`
            loaded_sections.insert(
                last_shndx,
                Arc::new(LoadedSection::new(
                    typ,
                    demangled,
                    Arc::clone(mapped_pages),
                    mapped_pages_offset,
                    virt_addr,
                    sec_size,
                    is_global,
                    new_crate.clone(),
                ))
            );

            if is_global {
                global_sections.insert(last_shndx);
            }
        
            last_shndx += 1;
        } // end of iterating over all symbol table entries


        // // Quick test to ensure that all .data and .bss sections are fully covered 
        // // by overlapping OBJECT symbols.
        // if !data_sections.is_empty() {
        //     warn!("Data sections for {:?}: {:?}", new_crate.upgrade().unwrap().lock_as_ref().crate_name, data_sections);
        //     if let Some(data_sec) = data_shndx.and_then(|i| loaded_sections.get(&i)) {
        //         warn!("\t .data sec size: {:#X}", data_sec.size());
        //         let mut data_symbols_start_vaddr = usize::MAX;
        //         let mut data_symbols_end_vaddr = 0;
        //         for sec in loaded_sections.values() {
        //             if sec.typ == SectionType::Data && sec.name.as_str() != SectionType::Data.name() {
        //                 warn!("\t\t data symbol: {:?}", sec);
        //                 data_symbols_start_vaddr = core::cmp::min(data_symbols_start_vaddr, sec.virt_addr.value());
        //                 data_symbols_end_vaddr = core::cmp::max(data_symbols_end_vaddr, sec.virt_addr.value() + sec.size);
        //             }
        //         }
        //         let total_size_of_data_symbols = data_symbols_end_vaddr - data_symbols_start_vaddr;
        //         if total_size_of_data_symbols != data_sec.size() {
        //             error!(".data section size {:#X} does not match total size of data symbols {:#X}", data_sec.size(), total_size_of_data_symbols);
        //         }
        //     }
        //     if let Some(bss_sec) = bss_shndx_and_offset.and_then(|(i, _off)| loaded_sections.get(&i)) {
        //         warn!("\t .bss sec size: {:#X}", bss_sec.size());
        //         let mut bss_symbols_start_vaddr = usize::MAX;
        //         let mut bss_symbols_end_vaddr = 0;
        //         for sec in loaded_sections.values() {
        //             if sec.typ == SectionType::Bss && sec.name.as_str() != SectionType::Bss.name() {
        //                 warn!("\t\t bss symbol: {:?}", sec);
        //                 bss_symbols_start_vaddr = core::cmp::min(bss_symbols_start_vaddr, sec.virt_addr.value());
        //                 bss_symbols_end_vaddr = core::cmp::max(bss_symbols_end_vaddr, sec.virt_addr.value() + sec.size);
        //             }
        //         }
        //         let total_size_of_bss_symbols = bss_symbols_end_vaddr - bss_symbols_start_vaddr;
        //         if total_size_of_bss_symbols != bss_sec.size() {
        //             error!(".bss section size {:#X} does not match total size of bss symbols {:#X}", bss_sec.size(), total_size_of_bss_symbols);
        //         }
        //     }
        // }

        Ok(SectionMetadata { 
            loaded_sections,
            global_sections,
            tls_sections,
            data_sections,
        })
    }


    /// An internal routine to load and populate the sections of a crate's object file
    /// if those sections have not been merged.
    /// 
    /// This is the "legacy" way to load sections, and is used by `load_crate_sections()`
    /// for object files that have **not** been modified by Theseus's special partial relinking script.
    /// 
    /// This works by iterating over all section headers in the object file
    /// and extracting the symbol names from each section name, which is quite slow. 
    fn load_crate_with_separate_sections(
        &self,
        elf_file:     &ElfFile,
        new_crate:    WeakCrateRef,
        text_pages:   Option<(Arc<Mutex<MappedPages>>, Range<VirtualAddress>)>,
        rodata_pages: Option<(Arc<Mutex<MappedPages>>, Range<VirtualAddress>)>,
        data_pages:   Option<(Arc<Mutex<MappedPages>>, Range<VirtualAddress>)>,
    ) -> Result<SectionMetadata, &'static str> { 
        
        // Check the symbol table to get the set of sections that are global (publicly visible).
        let global_sections: BTreeSet<Shndx> = {
            // For us to properly load the ELF file, it must NOT have been fully stripped,
            // meaning that it must still have its symbol table section. Otherwise, relocations will not work.
            let symtab = find_symbol_table(elf_file)?;

            let mut globals: BTreeSet<Shndx> = BTreeSet::new();
            use xmas_elf::symbol_table::Entry;
            for entry in symtab.iter() {
                // Include all symbols with "GLOBAL" binding, regardless of visibility.  
                if entry.get_binding() == Ok(xmas_elf::symbol_table::Binding::Global) {
                    match entry.get_type() {
                        Ok(xmas_elf::symbol_table::Type::Func 
                            | xmas_elf::symbol_table::Type::Object
                            | xmas_elf::symbol_table::Type::Tls) => {
                            globals.insert(entry.shndx() as Shndx);
                        }
                        _ => continue,
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
        // (-) It ends up wasting a some bytes here and there, but almost always under 100 bytes.
        //     If object file sections have been merged, no memory is wasted.
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
                    
        const TEXT_PREFIX:             &str = ".text.";
        const UNLIKELY_PREFIX:         &str = "unlikely."; // the full section prefix is ".text.unlikely."
        const RODATA_PREFIX:           &str = ".rodata.";
        const DATA_PREFIX:             &str = ".data.";
        const BSS_PREFIX:              &str = ".bss.";
        const TLS_DATA_PREFIX:         &str = ".tdata.";
        const TLS_BSS_PREFIX:          &str = ".tbss.";
        // const RELRO_PREFIX:            &str = "rel.ro.";
        const GCC_EXCEPT_TABLE_PREFIX: &str = ".gcc_except_table.";
        const EH_FRAME_NAME:           &str = ".eh_frame";

        /// A convenient macro to obtain the rest of the symbol name after its prefix,
        /// i.e., the characters after '.text', '.rodata', '.data', etc.
        /// 
        /// * If the name isn't long enough, the macro prints and returns an error str.
        /// * If the name isn't long enough but is an empty section (e.g., just ".text", ".rodata", etc)
        ///   this macro `continue`s to the next iteration of the loop.
        /// * The `$prefix` argument must be `const` so it can be `concat!()`-ed into a const &str.
        /// 
        /// Note: I'd prefer this to be a const function that accepts the prefix as a const &'static str,
        ///       but Rust does not support concat!()-ing const generic parameters yet.
        macro_rules! try_get_symbol_name_after_prefix {
            ($sec_name:ident, $prefix:ident) => (
                if let Some(name) = $sec_name.get($prefix.len() ..) {
                    name
                } else {
                    // Ignore special "empty" placeholder sections
                    match $sec_name {
                        ".text"   => continue,
                        ".rodata" => continue,
                        ".data"   => continue,
                        ".bss"    => continue,
                        _ => {
                            const ERROR_STR: &'static str = const_format::concatcp!(
                                "Failed to get the ", $prefix, 
                                " section's name after '", $prefix, "'"
                            );
                            error!("{}: {:?}", ERROR_STR, $sec_name);
                            return Err(ERROR_STR);
                        }
                    }
                }
            );
        }

        // this maps section header index (shndx) to LoadedSection
        let mut loaded_sections: HashMap<Shndx, StrongSectionRef> = HashMap::new(); 
        // the set of Shndxes for .data and .bss sections
        let mut data_sections: BTreeSet<Shndx> = BTreeSet::new();
        // the set of Shndxes for TLS sections (.tdata, .tbss)
        let mut tls_sections: BTreeSet<Shndx> = BTreeSet::new();

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
            let sec_name = match sec.get_name(elf_file) {
                Ok(name) => name,
                Err(_e) => {
                    error!("Couldn't get section name for section [{}]: {:?}\n    error: {}", shndx, sec, _e);
                    return Err("couldn't get section name");
                }
            };

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
                        } else {
                            // if it does not, we should NOT use it in place of the current section
                            sec
                        }
                    }
                    _ => {
                        error!("Couldn't get next section for zero-sized section {}", shndx);
                        return Err("couldn't get next section for a zero-sized section");
                    }
                }
            } else {
                // this is the normal case, a non-zero sized section, so just use the current section
                sec
            };

            // get the relevant section info, i.e., size, alignment, and data contents
            let sec_size  = sec.size()  as usize;
            let sec_align = sec.align() as usize;
            let is_write  = sec_flags & SHF_WRITE     == SHF_WRITE;
            let is_exec   = sec_flags & SHF_EXECINSTR == SHF_EXECINSTR;
            let is_tls    = sec_flags & SHF_TLS       == SHF_TLS;


            // First, check for executable sections, which can only be .text sections.
            if is_exec && !is_write {
                let is_global = global_sections.contains(&shndx);
                let name = try_get_symbol_name_after_prefix!(sec_name, TEXT_PREFIX);
                // Handle cold sections, which have a section prefix of ".text.unlikely."
                // Currently, we ignore the cold/hot designation in terms of placing a section in memory.
                // Note: we only *truly* have to do this for global sections, because other crates
                //       might depend on their correct section name after the ".text.unlikely." prefix.
                let name = if is_global && name.starts_with(UNLIKELY_PREFIX) {
                    name.get(UNLIKELY_PREFIX.len() ..).ok_or_else(|| {
                        error!("Failed to get the .text.unlikely. section's name: {:?}", sec_name);
                        "Failed to get the .text.unlikely. section's name after the prefix"
                    })?
                } else {
                    name
                };
                let demangled = demangle(name).to_string().as_str().into();

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
                            is_global,
                            new_crate.clone(),
                        ))
                    );
                }
                else {
                    return Err("BUG: ELF file contained a .text* section, but no text_pages were allocated");
                }
            }

            // Second, if not executable, handle TLS sections.
            // Although TLS sections have "WAT" flags (write, alloc, TLS),
            // we load TLS sections into the same read-only pages as other read-only sections (e.g., .rodata)
            // because they contain thread-local storage initializer data that is only read from.
            else if is_tls {
                // check if this TLS section is .bss or .data
                let is_bss = sec.get_type() == Ok(ShType::NoBits);
                let name = if is_bss {
                    try_get_symbol_name_after_prefix!(sec_name, TLS_BSS_PREFIX)
                } else {
                    try_get_symbol_name_after_prefix!(sec_name, TLS_DATA_PREFIX)
                };
                let demangled = demangle(name).to_string().as_str().into();

                if let Some((ref rp_ref, ref mut rp)) = read_only_pages_locked {
                    let (mapped_pages_offset, sec_typ) = if is_bss {
                        // Here: a TLS .tbss section has no actual content, so we use a max-value offset
                        // as a canary value to ensure it cannot be used to index into a MappedPages.
                        (usize::MAX, SectionType::TlsBss)
                    } else {
                        // Here: copy the TLS .tdata section's contents to the proper address in the read-only pages.
                        let dest_slice: &mut [u8] = rp.as_slice_mut(rodata_offset, sec_size)?;
                        match sec.get_data(elf_file) {
                            Ok(SectionData::Undefined(sec_data)) => dest_slice.copy_from_slice(sec_data),
                            _other => {
                                error!("load_crate_sections(): Couldn't get section data for TLS .tdata section [{}] {}: {:?}", shndx, sec_name, _other);
                                return Err("couldn't get section data in TLS .tdata section");
                            }
                        };
                        // As with all other normal sections, use the current offset for these read-only pages.
                        (rodata_offset, SectionType::TlsData)
                    };

                    let new_tls_section = LoadedSection::new(
                        sec_typ,
                        demangled,
                        Arc::clone(rp_ref),
                        mapped_pages_offset,
                        VirtualAddress::zero(), // will be replaced in `add_new_dynamic_tls_section()` below
                        sec_size,
                        global_sections.contains(&shndx),
                        new_crate.clone(),
                    );
                    // trace!("Loaded new TLS section: {:?}", new_tls_section);
                    
                    // Add the new TLS section to this namespace's initial TLS area,
                    // which will reserve/obtain a new offset into that TLS area which holds this section's data.
                    // This will also update the section's virtual address field to hold that offset value,
                    // which is used for relocation entries that ask for a section's offset from the TLS base.
                    let (_tls_offset, new_tls_section) = self.tls_initializer.lock()
                        .add_new_dynamic_tls_section(new_tls_section, sec_align)
                        .map_err(|_| "Failed to add new TLS section")?;

                    // trace!("\t --> updated new TLS section: {:?}", new_tls_section);
                    loaded_sections.insert(shndx, new_tls_section);
                    tls_sections.insert(shndx);

                    rodata_offset += sec_size.next_multiple_of(sec_align);
                }
                else {
                    return Err("no rodata_pages were allocated when handling TLS section");
                }
            }

            // Third, if not executable nor TLS, handle writable .data/.bss sections.
            else if is_write {
                // check if this section is .bss or .data
                let is_bss = sec.get_type() == Ok(ShType::NoBits);
                let name = if is_bss {
                    try_get_symbol_name_after_prefix!(sec_name, BSS_PREFIX)
                } else {
                    try_get_symbol_name_after_prefix!(sec_name, DATA_PREFIX)
                        // Currently, .rel.ro sections no longer exist in object files compiled for Theseus.
                        // .and_then(|name| {
                        //     if name.starts_with(RELRO_PREFIX) {
                        //         name.get(RELRO_PREFIX.len() ..)
                        //     } else {
                        //         Some(name)
                        //     }
                        // })
                };
                let demangled = demangle(name).to_string().as_str().into();
                
                if let Some((ref dp_ref, ref mut dp)) = read_write_pages_locked {
                    // here: we're ready to copy the data/bss section to the proper address
                    let dest_vaddr = dp.address_at_offset(data_offset)
                        .ok_or("BUG: data_offset wasn't within data_pages")?;
                    let dest_slice: &mut [u8] = dp.as_slice_mut(data_offset, sec_size)?;
                    match sec.get_data(elf_file) {
                        Ok(SectionData::Undefined(sec_data)) => dest_slice.copy_from_slice(sec_data),
                        Ok(SectionData::Empty) => dest_slice.fill(0),
                        _other => {
                            error!("load_crate_sections(): Couldn't get section data for .data section [{}] {}: {:?}", shndx, sec_name, _other);
                            return Err("couldn't get section data in .data section");
                        }
                    }
                    
                    loaded_sections.insert(
                        shndx,
                        Arc::new(LoadedSection::new(
                            if is_bss { SectionType::Bss } else { SectionType::Data },
                            demangled,
                            Arc::clone(dp_ref),
                            data_offset,
                            dest_vaddr,
                            sec_size,
                            global_sections.contains(&shndx),
                            new_crate.clone(),
                        ))
                    );
                    data_sections.insert(shndx);

                    data_offset += sec_size.next_multiple_of(sec_align);
                }
                else {
                    return Err("no data_pages were allocated for .data/.bss section");
                }
            }

            // Fourth, if neither executable nor TLS nor writable, handle .rodata sections.
            else if sec_name.starts_with(RODATA_PREFIX) {
                let name = try_get_symbol_name_after_prefix!(sec_name, RODATA_PREFIX);
                let demangled = demangle(name).to_string().as_str().into();

                if let Some((ref rp_ref, ref mut rp)) = read_only_pages_locked {
                    // here: we're ready to copy the rodata section to the proper address
                    let dest_vaddr = rp.address_at_offset(rodata_offset)
                        .ok_or("BUG: rodata_offset wasn't within rodata_mapped_pages")?;
                    let dest_slice: &mut [u8] = rp.as_slice_mut(rodata_offset, sec_size)?;
                    match sec.get_data(elf_file) {
                        Ok(SectionData::Undefined(sec_data)) => dest_slice.copy_from_slice(sec_data),
                        Ok(SectionData::Empty) => dest_slice.fill(0),
                        _other => {
                            error!("load_crate_sections(): Couldn't get section data for .rodata section [{}] {}: {:?}", shndx, sec_name, _other);
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
                            new_crate.clone(),
                        ))
                    );

                    rodata_offset += sec_size.next_multiple_of(sec_align);
                }
                else {
                    return Err("no rodata_pages were allocated when handling .rodata section");
                }
            }

            // Fifth, if neither executable nor TLS nor writable nor .rodata, handle the `.gcc_except_table` sections
            else if sec_name.starts_with(GCC_EXCEPT_TABLE_PREFIX) {
                // We don't need to waste space keeping the name of the `.gcc_exept_table` section, 
                // because that name is irrelevant and will never be used.
                //
                // let name = try_get_symbol_name_after_prefix!(sec_name, GCC_EXCEPT_TABLE_PREFIX);
                // let demangled = demangle(name).to_string().into();

                // gcc_except_table sections are read-only, so we put them in the .rodata pages
                if let Some((ref rp_ref, ref mut rp)) = read_only_pages_locked {
                    // here: we're ready to copy the rodata section to the proper address
                    let dest_vaddr = rp.address_at_offset(rodata_offset)
                        .ok_or("BUG: rodata_offset wasn't within rodata_mapped_pages")?;
                    let dest_slice: &mut [u8]  = rp.as_slice_mut(rodata_offset, sec_size)?;
                    match sec.get_data(elf_file) {
                        Ok(SectionData::Undefined(sec_data)) => dest_slice.copy_from_slice(sec_data),
                        Ok(SectionData::Empty) => dest_slice.fill(0),
                        _other => {
                            error!("load_crate_sections(): Couldn't get section data for .gcc_except_table section [{}] {}: {:?}", shndx, sec_name, _other);
                            return Err("couldn't get section data in .gcc_except_table section");
                        }
                    }

                    let typ = SectionType::GccExceptTable;
                    loaded_sections.insert(
                        shndx, 
                        Arc::new(LoadedSection::new(
                            typ,
                            section_name_str_ref(&typ),
                            Arc::clone(rp_ref),
                            rodata_offset,
                            dest_vaddr,
                            sec_size,
                            false, // .gcc_except_table sections are never globally visible,
                            new_crate.clone(),
                        ))
                    );

                    rodata_offset += sec_size.next_multiple_of(sec_align);
                }
                else {
                    return Err("no rodata_pages were allocated when handling .gcc_except_table");
                }
            }

            // Fifth, if neither executable nor TLS nor writable nor .rodata nor .gcc_except_table, handle the `.eh_frame` section
            else if sec_name == EH_FRAME_NAME {
                // The eh_frame section is read-only, so we put it in the .rodata pages
                if let Some((ref rp_ref, ref mut rp)) = read_only_pages_locked {
                    // here: we're ready to copy the rodata section to the proper address
                    let dest_vaddr = rp.address_at_offset(rodata_offset)
                        .ok_or("BUG: rodata_offset wasn't within rodata_mapped_pages")?;
                    let dest_slice: &mut [u8]  = rp.as_slice_mut(rodata_offset, sec_size)?;
                    match sec.get_data(elf_file) {
                        Ok(SectionData::Undefined(sec_data)) => dest_slice.copy_from_slice(sec_data),
                        Ok(SectionData::Empty) => dest_slice.fill(0),
                        _other => {
                            error!("load_crate_sections(): Couldn't get section data for .eh_frame section [{}] {}: {:?}", shndx, sec_name, _other);
                            return Err("couldn't get section data in .eh_frame section");
                        }
                    }

                    let typ = SectionType::EhFrame;
                    loaded_sections.insert(
                        shndx, 
                        Arc::new(LoadedSection::new(
                            typ,
                            section_name_str_ref(&typ),
                            Arc::clone(rp_ref),
                            rodata_offset,
                            dest_vaddr,
                            sec_size,
                            false, // .eh_frame section is not globally visible,
                            new_crate.clone(),
                        ))
                    );

                    rodata_offset += sec_size.next_multiple_of(sec_align);
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

        Ok(SectionMetadata { 
            loaded_sections,
            global_sections,
            tls_sections,
            data_sections,
        })
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
        let symtab = find_symbol_table(elf_file)?;

        // Fix up the sections that were just loaded, using proper relocation info.
        // Iterate over every non-zero relocation section in the file
        for sec in elf_file.section_iter().filter(|sec| sec.get_type() == Ok(ShType::Rela) && sec.size() != 0) {
            use xmas_elf::sections::SectionData::Rela64;
            if verbose_log { 
                trace!("Found Rela section name: {:?}, type: {:?}, target_sec_index: {:?}", 
                sec.get_name(elf_file), sec.get_type(), sec.info()); 
            }

            // Debug sections are handled separately
            if let Ok(name) = sec.get_name(elf_file) {
                if name.starts_with(".rela.debug") { // ignore debug special sections for now
                    continue;
                }
            }

            let rela_array = match sec.get_data(elf_file) {
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
                error!("ELF file error: target section was not loaded for Rela section {:?}!", sec.get_name(elf_file));
                "target section was not loaded for Rela section"
            })?; 

            let mut target_sec_data_was_modified = false;
            
            let mut target_sec_dependencies: Vec<StrongDependency> = Vec::new();
            #[cfg(internal_deps)]
            let mut target_sec_internal_dependencies: Vec<InternalDependency> = Vec::new();
            {
                let mut target_sec_mapped_pages = target_sec.mapped_pages.lock();
                let target_sec_slice: &mut [u8] = target_sec_mapped_pages.as_slice_mut(
                    0,
                    target_sec.mapped_pages_offset + target_sec.size,
                )?;

                // iterate through each relocation entry in the relocation array for the target_sec
                for rela_entry in rela_array {
                    if verbose_log { 
                        trace!("      Rela64 offset: {:#X}, addend: {:#X}, symtab_index: {}, type: {:#X}", 
                            rela_entry.get_offset(), rela_entry.get_addend(), rela_entry.get_symbol_table_index(), rela_entry.get_type());
                    }

                    use xmas_elf::symbol_table::Entry;
                    let source_sec_entry = &symtab[rela_entry.get_symbol_table_index() as usize];
                    let source_sec_shndx = source_sec_entry.shndx() as usize; 
                    let source_sec_value = source_sec_entry.value() as usize;
                    if verbose_log { 
                        let source_sec_header_name = source_sec_entry.get_section_header(elf_file, rela_entry.get_symbol_table_index() as usize)
                            .and_then(|s| s.get_name(elf_file));
                        trace!("             relevant section [{}]: {:?}, value: {:#X}", source_sec_shndx, source_sec_header_name, source_sec_value);
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
                            if let Ok(source_sec_name) = source_sec_entry.get_name(elf_file) {
                                const DATARELRO: &str = ".data.rel.ro.";
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
                                    .get_section_header(elf_file, rela_entry.get_symbol_table_index() as usize)
                                    .and_then(|s| s.get_name(elf_file));
                                error!("Couldn't get name of source section [{}] {:?}, needed for non-local relocation entry", source_sec_shndx, _source_sec_header);
                                Err("Couldn't get source section's name, needed for non-local relocation entry")
                            }
                        }
                    }?;

                    let relocation_entry = RelocationEntry::from_elf_relocation(rela_entry);
                    write_relocation(
                        relocation_entry,
                        target_sec_slice,
                        target_sec.mapped_pages_offset,
                        source_sec.virt_addr + source_sec_value,
                        verbose_log
                    )?;
                    target_sec_data_was_modified = true;

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
                            section: Arc::downgrade(target_sec),
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

            // If the target section of the relocation was a TLS section, 
            // that TLS section's initializer data has now changed.
            // Thus, we need to invalidate the TLS initializer area's cached data.
            if target_sec_data_was_modified && 
                (target_sec.typ == SectionType::TlsData || target_sec.typ == SectionType::TlsBss)
            {
                // debug!("Invalidating TlsInitializer due to relocation written to section {:?}", &*target_sec);
                self.tls_initializer.lock().invalidate();
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
                    && sec.typ == SectionType::Rodata
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
        new_section_key: StrRef,
        new_section: &StrongSectionRef,
        log_replacements: bool,
    ) -> bool {
        match existing_symbol_map.entry(new_section_key) {
            qp_trie::Entry::Occupied(mut old_val) => {
                if log_replacements {
                    if let Some(old_sec) = old_val.get().upgrade() {
                        // debug!("       add_symbol(): replacing section: old: {:?}, new: {:?}", old_sec, new_section);
                        if new_section.size != old_sec.size {
                            warn!("Unexpectedly replacing differently-sized section: old: ({}B) {:?}, new: ({}B) {:?}", old_sec.size, old_sec.name, new_section.size, new_section.name);
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
            let condition = filter_func(sec) && sec.global;
            if condition {
                // trace!("add_symbols_filtered(): adding symbol {:?}", sec);
                let added = CrateNamespace::add_symbol(&mut existing_map, sec.name.clone(), sec, log_replacements);
                if added {
                    count += 1;
                }
            }
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
    /// By default, only executable sections (`.text`) are searched, since the typical use case 
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
    /// This can obtain the lock on every crate in this namespace and its recursive namespaces, 
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

        // We try to find the *most specific* section that contains the `virt_addr`.
        // If sections have been merged, there will be a merged section that contains `virt_addr`,
        // but also potentially a section for an individual symbol that is a better, more descriptive match.
        // For example, `my_crate::foo()` will exist in `my_crate`'s `.text` section, 
        // but may also contained in `my_crate`'s `foo` section, which is a better return value here.
        let mut merged_section_and_offset = None;

        // Second, we find the section in that crate that contains the address.
        for sec in crate_locked.sections.values() {
            // .text sections are always included, other sections are included if requested.
            let eligible_section = sec.typ == SectionType::Text || search_all_section_types;
            
            // If the section's address bounds contain the address, then we've found it.
            // Only a single section can contain the address, so it's safe to stop once we've found a match.
            if eligible_section
                && sec.virt_addr <= virt_addr
                && virt_addr.value() < (sec.virt_addr.value() + sec.size)
            {
                let offset = virt_addr.value() - sec.virt_addr.value();
                merged_section_and_offset = Some((sec.clone(), offset));
                
                if sec.name.as_str() == sec.typ.name() {
                    // If this section is a merged section, it will have the standard name
                    // for its section type, e.g., ".text", ".data", ".rodata", etc.
                    // Thus, we should keep looking to find a better, more specific symbol section.
                } else {
                    // If the name is *not* that standard name, then we have found the
                    // "more specific" symbol's section that contains this `virt_addr`.
                    // We can stop looking because only one symbol section can possibly contain it.
                    break;
                }
            }
        }
        merged_section_and_offset
    }

    /// Like [`get_symbol()`](#method.get_symbol), but also returns the exact `CrateNamespace` where the symbol was found.
    pub fn get_symbol_and_namespace(&self, demangled_full_symbol: &str) -> Option<(WeakSectionRef, &CrateNamespace)> {
        let weak_symbol = self.symbol_map.lock().get(demangled_full_symbol.as_bytes()).cloned();
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

        // Finally, try to load the crate that may contain the missing symbol.
        if let Some(weak_sec) = self.load_crate_for_missing_symbol(demangled_full_symbol, temp_backup_namespace, kernel_mmi_ref, verbose_log) {
            weak_sec
        } else {
            #[cfg(not(loscd_eval))]
            warn!("Symbol \"{}\" not found. Try loading the specific crate manually first.", demangled_full_symbol);
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
                        fuzzy_matches.iter().map(|tup| &tup.0).collect::<Vec<_>>()
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
        self.crate_tree.lock().insert(parent_crate_name, parent_crate_ref);
        Some(sec)
    }


    /// Attempts to find and load the crate that may contain the given `demangled_full_symbol`. 
    /// 
    /// If successful, the new crate is loaded into this `CrateNamespace` and the symbol's section is returned.
    /// If this namespace does not contain any matching crates, its recursive namespaces are searched as well.
    /// 
    /// This approach only works for mangled symbols that contain a crate name, such as "my_crate::foo". 
    /// If "foo()" was marked no_mangle, then we don't know which crate to load because there is no "my_crate::" prefix before it.
    /// 
    /// Note: while attempting to find the missing `demangled_full_symbol`, this function may end up
    /// loading *multiple* crates into this `CrateNamespace` or its recursive namespaces, due to two reasons:
    /// 1. The `demangled_full_symbol` may have multiple crate prefixes within it.
    ///    * For example, `<page_allocator::AllocatedPages as core::ops::drop::Drop>::drop::h55e0a4c312ccdd63`
    ///      contains two possible crate prefixes: `page_allocator` and `core`.
    /// 2. There may be multiple versions of a single crate.
    /// 
    /// Possible crates are iteratively loaded and searched until the missing symbol is found.
    /// Currently, crates that were loaded but did *not* contain the missing symbol are *not* unloaded,
    /// but you could manually unload them later with no adverse effects to reclaim memory.
    /// 
    /// This is the final attempt to find a symbol within [`CrateNamespace::get_symbol_or_load()`].
    fn load_crate_for_missing_symbol(
        &self,
        demangled_full_symbol: &str,
        temp_backup_namespace: Option<&CrateNamespace>,
        kernel_mmi_ref: &MmiRef,
        verbose_log: bool,
    ) -> Option<WeakSectionRef> {
        // Some symbols may have multiple potential containing crates, so we try to load each one to find the missing symbol.
        for potential_crate_name in get_containing_crate_name(demangled_full_symbol) {
            let potential_crate_name = format!("{potential_crate_name}-");
 
            // Try to find and load the missing crate object file from this namespace's directory or its recursive namespace's directory,
            // (or from the backup namespace's directory set).
            // The object files from the recursive namespace(s) are appended after the files in the initial namespace,
            // so they'll only be searched if the symbol isn't found in the current namespace.
            for (potential_crate_file, ns_of_crate_file) in self.method_get_crate_object_files_starting_with(&potential_crate_name) {
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
        }

        warn!("Couldn't find/load crate(s) that may contain the missing symbol {:?}", demangled_full_symbol);
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
            .iter_prefix(symbol_prefix.as_bytes())
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
            .iter_prefix(symbol_prefix.as_bytes())
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
        let mut iter = map.iter_prefix(symbol_prefix.as_bytes()).map(|tuple| tuple.1);
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
            syms = format!("{syms}\n{syms_recursive}");
        }

        syms
    }
}


/// A convenience wrapper around a new crate's data items that are generated
/// when iterating over and loading its sections.
struct SectionMetadata {
    loaded_sections: HashMap<usize, Arc<LoadedSection>>,
    global_sections: BTreeSet<usize>,
    tls_sections:    BTreeSet<usize>,
    data_sections:   BTreeSet<usize>,
}


/// A convenience wrapper for a set of the three possible types of `MappedPages`
/// that can be allocated and mapped for a single `LoadedCrate`. 
struct SectionPages {
    /// MappedPages that will hold any and all executable sections: `.text`
    /// and their bounds expressed as `VirtualAddress`es.
    executable_pages: Option<(MappedPages, Range<VirtualAddress>)>,
    /// MappedPages that will hold any and all read-only sections: `.rodata`, `.eh_frame`, `.gcc_except_table`
    /// and their bounds expressed as `VirtualAddress`es.
    read_only_pages: Option<(MappedPages, Range<VirtualAddress>)>,
    /// MappedPages that will hold any and all read-write sections: `.data` and `.bss`
    /// and their bounds expressed as `VirtualAddress`es.
    read_write_pages: Option<(MappedPages, Range<VirtualAddress>)>,
}


/// Allocates and maps memory sufficient to hold the sections that are found in the given `ElfFile`.
/// Only sections that are marked "allocated" (`ALLOC`) in the ELF object file will contribute to the mappings' sizes.
fn allocate_section_pages(elf_file: &ElfFile, kernel_mmi_ref: &MmiRef) -> Result<SectionPages, &'static str> {
    // Calculate how many bytes (and thus how many pages) we need for each of the three section types.
    //
    // If there are multiple .text sections, they will all exist at the beginning of the object file,
    // so we simply find the end of the last .text section and use that as the end bounds.
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
                // warn!("Unlikely scenario: found zero-sized sec {:X?}", sec);
                let next_sec = elf_file.section_header((shndx + 1) as u16)
                    .map_err(|_| "couldn't get next section for a zero-sized section")?;
                if next_sec.offset() == sec.offset() {
                    // warn!("Using next_sec {:X?} instead of zero-sized sec {:X?}", next_sec, sec);
                    next_sec
                } else {
                    sec
                }
            } else {
                sec
            };

            let size = sec.size() as usize;
            let align = sec.align() as usize;
            let addend = size.next_multiple_of(align);

            // filter flags for ones we care about (we already checked that it's loaded (SHF_ALLOC))
            let is_write = sec_flags & SHF_WRITE     == SHF_WRITE;
            let is_exec  = sec_flags & SHF_EXECINSTR == SHF_EXECINSTR;
            let is_tls   = sec_flags & SHF_TLS       == SHF_TLS;
            // trace!("  Looking at sec {:?}, size {:#X}, align {:#X} --> addend {:#X}", sec.get_name(elf_file), size, align, addend);
            if is_exec {
                // this includes only .text sections
                text_max_offset = core::cmp::max(text_max_offset, (sec.offset() as usize) + addend);
            }
            else if is_tls {
                // TLS sections are included as part of read-only pages,
                // but we only need to allocate space for .tdata sections, not .tbss.
                if sec.get_type() == Ok(ShType::ProgBits) {
                    ro_bytes += addend;
                }
                // Ignore .tbss sections, which have type `NoBits`.
            }
            else if is_write {
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

    // trace!("\n\texec_bytes: {exec_bytes} {exec_bytes:#X}\n\tro_bytes:   {ro_bytes} {ro_bytes:#X}\n\trw_bytes:   {rw_bytes} {rw_bytes:#X}");

    // Allocate contiguous virtual memory pages for each section and map them to random frames as writable.
    // We must allocate these pages separately because they will have different flags later.
    let executable_pages = if exec_bytes > 0 { Some(allocate_and_map_as_writable(exec_bytes, TEXT_SECTION_FLAGS,     kernel_mmi_ref)?) } else { None };
    let read_only_pages  = if ro_bytes   > 0 { Some(allocate_and_map_as_writable(ro_bytes,   RODATA_SECTION_FLAGS,   kernel_mmi_ref)?) } else { None };
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


/// A convenience function for allocating virtual pages and mapping them to random physical frames. 
/// 
/// The returned `MappedPages` will be at least as large as `size_in_bytes`,
/// rounded up to the nearest `Page` size, 
/// and is mapped as writable along with the other specified `flags`
/// to ensure we can copy content into it.
fn allocate_and_map_as_writable(
    size_in_bytes: usize,
    flags: PteFlags,
    kernel_mmi_ref: &MmiRef,
) -> Result<MappedPages, &'static str> {
    let allocated_pages = allocate_pages_by_bytes(size_in_bytes)
        .ok_or("Couldn't allocate_pages_by_bytes, out of virtual address space")?;
    kernel_mmi_ref.lock().page_table.map_allocated_pages(
        allocated_pages,
        flags.valid(true).writable(true)
    )
}


#[allow(dead_code)]
fn dump_dependent_crates(krate: &LoadedCrate, prefix: String) {
	for weak_crate_ref in krate.crates_dependent_on_me() {
		let strong_crate_ref = weak_crate_ref.upgrade().unwrap();
        let strong_crate = strong_crate_ref.lock_as_ref();
		debug!("{}{}", prefix, strong_crate.crate_name);
		dump_dependent_crates(&strong_crate, format!("{prefix}  "));
	}
}


#[allow(dead_code)]
fn dump_weak_dependents(sec: &LoadedSection, prefix: String) {
    let sec_inner = sec.inner.read();
	if !sec_inner.sections_dependent_on_me.is_empty() {
		debug!("{}Section \"{}\": sections dependent on me (weak dependents):", prefix, sec.name);
		for weak_dep in &sec_inner.sections_dependent_on_me {
			if let Some(wds) = weak_dep.section.upgrade() {
				let prefix = format!("{prefix}  "); // add two spaces of indentation to the prefix
				dump_weak_dependents(&wds, prefix);
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
        .find(|sec| sec.get_type() == Ok(ShType::SymTab))
        .ok_or("no symtab section")
        .and_then(|s| s.get_data(elf_file));

    match symtab_data {
        Ok(SymbolTable64(symtab)) => Ok(symtab),
        _ => {
            Err("no symbol table found. Was file stripped?")
        }
    }
}


/// A Thread-Local Storage (TLS) area data "image" that is used
/// to initialize a new `Task`'s TLS area.
#[derive(Debug, Clone)]
pub(crate) struct TlsInitializer {
    /// The cached data image (with blank space for the TLS self pointer).
    /// This is used to avoid unnecessarily re-generating the TLS data image
    /// every time a new task is spawned if no TLS data sections have been added.
    data_cache: Vec<u8>,
    /// The status of the above `data_cache`: whether it is ready to be used
    /// immediately or needs to be regenerated.
    cache_status: CacheStatus,
    /// The set of TLS data sections that are defined at link time
    /// and come from the statically-linked base kernel image (the nano_core).
    /// According to the x86_64 TLS ABI, these exist at **negative** offsets
    /// from the TLS self pointer, i.e., they exist **before** the TLS self pointer in memory.
    /// Thus, their actual location in memory depends on the size of **all** static TLS data sections.
    /// For example, the last section in this set (with the highest offset) will be placed
    /// right before the TLS self pointer in memory. 
    static_section_offsets:  RangeMap<usize, StrongSectionRefWrapper>,
    /// The ending offset (an exclusive range end bound) of the last TLS section
    /// in the above set of `static_section_offsets`.
    /// This is the offset where the TLS self pointer exists.
    end_of_static_sections: usize,
    /// The set of TLS data sections that come from dynamically-loaded crate object files.
    /// We can control and arbitrarily assign their offsets, and thus,
    /// we place all of these sections **after** the TLS self pointer in memory.
    /// For example, the first section in this set (with an offset of `0`) will be place
    /// right after the TLS self pointer in memory.
    dynamic_section_offsets: RangeMap<usize, StrongSectionRefWrapper>,
    /// The ending offset (an exclusive range end bound) of the last TLS section
    /// in the above set of `dynamic_section_offsets`.
    end_of_dynamic_sections: usize,
} 

const POINTER_SIZE: usize = size_of::<usize>();

impl TlsInitializer {
    /// Creates an empty TLS initializer with no TLS data sections.
    const fn empty() -> TlsInitializer {
        TlsInitializer {
            // The data image will be generated lazily on the next request to use it.
            data_cache: Vec::new(),
            cache_status: CacheStatus::Invalidated,
            static_section_offsets: RangeMap::new(),
            end_of_static_sections: 0,
            dynamic_section_offsets: RangeMap::new(),
            end_of_dynamic_sections: 0,
        }
    }

    /// Add a TLS section that has pre-determined offset, e.g.,
    /// one that was specified in the statically-linked base kernel image.
    ///
    /// This function modifies the `tls_section`'s starting virtual address field
    /// to hold the proper value such that this `tls_section` can be correctly used
    /// as the source of a relocation calculation (e.g., when another section depends on it).
    /// That value will be a negative offset from the end of all the static TLS sections,
    /// i.e., where the TLS self pointer exists in memory.
    ///
    /// ## Arguments
    /// * `tls_section`: the TLS section present in base kernel image.
    /// * `offset`: the offset of this section as determined by the linker.
    ///    This corresponds to the "value" of this section's symbol in the ELF file.
    /// * `total_static_tls_size`: the total size of all statically-known TLS sections,
    ///    including both TLS BSS (`.tbss`) and TLS data (`.tdata`) sections.
    ///
    /// ## Return
    /// * A reference to the newly added and properly modified section, if successful.
    /// * An error if inserting the given `tls_section` at the given `offset`
    ///   would overlap with an existing section. 
    ///   An error occurring here would indicate a link-time bug 
    ///   or a bug in the symbol parsing code that invokes this function.
    pub(crate) fn add_existing_static_tls_section(
        &mut self,
        mut tls_section: LoadedSection,
        offset: usize,
        total_static_tls_size: usize,
    ) -> Result<StrongSectionRef, ()> {
        let range = offset .. (offset + tls_section.size);
        if self.static_section_offsets.contains_key(&range.start) || 
            self.static_section_offsets.contains_key(&(range.end - 1))
        {
            return Err(());
        }

        // Calculate the new value of this section's virtual address based on its offset.
        let starting_offset = (total_static_tls_size - offset).wrapping_neg();
        tls_section.virt_addr = VirtualAddress::new(starting_offset).ok_or(())?;
        self.end_of_static_sections = max(self.end_of_static_sections, range.end);
        let section_ref = Arc::new(tls_section);
        self.static_section_offsets.insert(range, StrongSectionRefWrapper(section_ref.clone()));
        self.cache_status = CacheStatus::Invalidated;
        Ok(section_ref)
    }

    /// Inserts the given `section` into this TLS area at the next index
    /// (i.e., offset into the TLS area) where the section will fit.
    /// 
    /// This also modifies the virtual address field of the given `section`
    /// to hold the value of that offset, which is necessary for relocation entries
    /// that depend on this section.
    /// 
    /// Note: this will never return an index/offset value less than `size_of::<usize>()`,
    /// (`8` on a 64-bit machine), as the first slot is reserved for the TLS self pointer.
    /// 
    /// Returns a tuple of:
    /// 1. The index at which the new section was inserted, 
    ///    which is the offset from the beginning of the TLS area where the section data starts.
    /// 2. The modified section as a `StrongSectionRef`.
    /// 
    /// Returns an Error if there is no remaining space that can fit the section.
    pub(crate) fn add_new_dynamic_tls_section(
        &mut self,
        mut section: LoadedSection,
        alignment: usize,
    ) -> Result<(usize, StrongSectionRef), ()> {
        let mut start_index = None;
        // Find the next "gap" big enough to fit the new TLS section, 
        // skipping the first `POINTER_SIZE` bytes, which are reserved for the TLS self pointer.
        let range_after_tls_self_pointer = POINTER_SIZE .. usize::MAX;
        for gap in self.dynamic_section_offsets.gaps(&range_after_tls_self_pointer) {
            let aligned_start = gap.start.next_multiple_of(alignment);
            if aligned_start + section.size <= gap.end {
                start_index = Some(aligned_start);
                break;
            }
        }

        let start = start_index.ok_or(())?;
        let range = start .. (start + section.size);
        section.virt_addr = VirtualAddress::new(range.start).ok_or(())?;
        let section_ref = Arc::new(section);
        self.end_of_dynamic_sections = max(self.end_of_dynamic_sections, range.end);
        self.dynamic_section_offsets.insert(range, StrongSectionRefWrapper(section_ref.clone()));
        // Now that we've added a new section, the cached data is invalid.
        self.cache_status = CacheStatus::Invalidated;
        Ok((start, section_ref))
    }

    /// Invalidates the cached data image in this `TlsInitializer` area.
    /// 
    /// This is useful for when a TLS section's data has been modified,
    /// e.g., while performing relocations, 
    /// and thus the data image needs to be re-created by re-reading the section data.
    pub fn invalidate(&mut self) {
        self.cache_status = CacheStatus::Invalidated;
    }

    /// Returns a new copy of the TLS data image.
    /// 
    /// This function lazily generates the TLS image data on demand, if needed.
    pub(crate) fn get_data(&mut self) -> TlsDataImage {
        let total_section_size = self.end_of_static_sections + self.end_of_dynamic_sections;
        let required_capacity = if total_section_size > 0 { total_section_size + POINTER_SIZE } else { 0 };
        if required_capacity == 0 {
            return TlsDataImage { _data: None, ptr: 0 };
        }

        // An internal function that iterates over all TLS sections and copies their data into the new data image.
        fn copy_tls_section_data(
            new_data: &mut Vec<u8>,
            section_offsets: &RangeMap<usize, StrongSectionRefWrapper>,
            end_of_previous_range: &mut usize,
        ) {
            for (range, sec) in section_offsets.iter() {
                // Insert padding bytes into the data vec to ensure the section data is inserted at the correct index.
                let num_padding_bytes = range.start.saturating_sub(*end_of_previous_range);
                new_data.extend(core::iter::repeat(0).take(num_padding_bytes));

                // Insert the section data into the new data vec.
                if sec.typ == SectionType::TlsData {
                    let sec_mp = sec.mapped_pages.lock();
                    let sec_data: &[u8] = sec_mp.as_slice(sec.mapped_pages_offset, sec.size).unwrap();
                    new_data.extend_from_slice(sec_data);
                } else {
                    // For TLS BSS sections (.tbss), fill the section size with all zeroes.
                    new_data.extend(core::iter::repeat(0).take(sec.size));
                }
                *end_of_previous_range = range.end;
            }
        }

        if self.cache_status == CacheStatus::Invalidated {
            // debug!("TlsInitializer was invalidated, re-generating data.\n{:#X?}", self);

            // On some architectures, such as x86_64, the ABI convention REQUIRES that
            // the TLS area data starts with a pointer to itself (the TLS self pointer).
            // Also, all data for "existing" (statically-linked) TLS sections must
            // come *before* the TLS self pointer, i.e., at negative offsets from the TLS self pointer.
            // Thus, we handle that here by appending space for a pointer (one `usize`)
            // to the `new_data` vector after we insert the static TLS data sections.
            // The location of the new pointer value is the conceptual "start" of the TLS image,
            // and that's what should be used for the value of the TLS register (e.g., `FS_BASE` MSR on x86_64).
            let mut new_data: Vec<u8> = Vec::with_capacity(required_capacity);
            
            // Iterate through all static TLS sections and copy their data into the new data image.
            let mut end_of_previous_range: usize = 0;
            copy_tls_section_data(&mut new_data, &self.static_section_offsets, &mut end_of_previous_range);
            assert_eq!(end_of_previous_range, self.end_of_static_sections);

            // Append space for the TLS self pointer immediately after the end of the last static TLS data section;
            // its actual value will be filled in later (in `get_data()`) after a new copy of the TLS data image is made.
            new_data.extend_from_slice(&[0u8; POINTER_SIZE]);

            // Iterate through all dynamic TLS sections and copy their data into the new data image.
            end_of_previous_range = POINTER_SIZE; // we already pushed room for the TLS self pointer above.
            copy_tls_section_data(&mut new_data, &self.dynamic_section_offsets, &mut end_of_previous_range);
            if self.end_of_dynamic_sections != 0 {
                // this assertion only makes sense if there are any dynamic sections
                assert_eq!(end_of_previous_range, self.end_of_dynamic_sections);
            }

            self.data_cache = new_data;
            self.cache_status = CacheStatus::Fresh;
        }

        // Here, the `data_cache` is guaranteed to be fresh and ready to use.
        let mut data_copy: Box<[u8]> = self.data_cache.as_slice().into();
        // Every time we create a new copy of the TLS data image, we have to re-calculate
        // and re-assign the TLS self pointer value (located after the static TLS section data),
        // because the virtual address of that new TLS data image copy will be unique.
        // Note that we only do this if the data_copy actually contains any TLS data.
        let self_ptr_offset = self.end_of_static_sections;
        if let Some(dest_slice) = data_copy.get_mut(self_ptr_offset .. (self_ptr_offset + POINTER_SIZE)) {
            let tls_self_ptr_value = dest_slice.as_ptr() as usize;
            dest_slice.copy_from_slice(&tls_self_ptr_value.to_ne_bytes());
            TlsDataImage {
                _data: Some(data_copy),
                ptr:   tls_self_ptr_value,
            }
        } else {
            panic!("BUG: offset of TLS self pointer was out of bounds in the TLS data image:\n{:02X?}", data_copy);
        }
    }
}

/// An initialized TLS area data image ready to be used by a new task.
/// 
/// The data is opaque, but one can obtain a pointer to the TLS area.
/// 
/// The enclosed opaque data is stored as a boxed slice (`Box<[u8]>`)
/// instead of a vector (`Vec<u8>`) because it is instantiated once upon task creation
/// and should never be expanded or shrunk.
/// 
/// The data is "immutable" with respect to Theseus task management functions
/// at the language level.
/// However, the data within this TLS area will be modified directly by code
/// that executes "in" this task, e.g., instructions that access the current TLS area.
#[derive(Debug)]
pub struct TlsDataImage {
    // The data is wrapped in an Option to avoid allocating an empty boxed slice
    // when there are no TLS data sections.
    _data: Option<Box<[u8]>>,
    ptr:   usize,
}
impl TlsDataImage {
    /// Returns the value of the TLS self pointer for this TLS data image.
    /// If it has no TLS data sections, the returned value will be zero.
    #[inline(always)]
    pub fn pointer_value(&self) -> usize {
        self.ptr
    }
}


/// The status of a cached TLS area data image.
#[derive(Debug, Clone, PartialEq, Eq)]
enum CacheStatus {
    /// The cached data image is up to date and can be used immediately.
    Fresh,
    /// The cached data image is out of date and needs to be regenerated.
    Invalidated,
}


/// A wrapper around a `StrongSectionRef` that implements `PartialEq` and `Eq` 
/// so we can use it in a `RangeMap`.
#[derive(Debug, Clone)]
struct StrongSectionRefWrapper(StrongSectionRef);
impl Deref for StrongSectionRefWrapper {
    type Target = StrongSectionRef;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl PartialEq for StrongSectionRefWrapper {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for StrongSectionRefWrapper { }

/// Convenience function for calculating the address range of a MappedPages object.
fn mp_range(mp_ref: &Arc<Mutex<MappedPages>>) -> Range<VirtualAddress> {
    let mp = mp_ref.lock();
    mp.start_address()..(mp.start_address() + mp.size_in_bytes())
}

