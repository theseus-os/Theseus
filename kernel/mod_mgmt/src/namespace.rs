//! This crate contains structures related to crate namespaces and their symbol maps. 


use alloc::{String, Vec, BTreeMap};
use alloc::btree_map::Entry;
use alloc::arc::{Arc, Weak};
use spin::{Mutex, RwLock};

use memory::{get_module, MemoryManagementInfo};
use metadata::*;


/// A "system map" from a fully-qualified demangled symbol String  
/// to weak reference to a `LoadedSection`.
/// This is used for relocations, and for looking up function names.
pub type SymbolMap = BTreeMap<String, WeakSectionRef>;


/// This struct represents a namespace of crates and their "global" (publicly-visible) symbols.
/// A crate namespace struct is basically a container around many crates 
/// that have all been loaded and linked against each other, 
/// completely separate and in isolation from any other crate namespace 
/// (although a given crate may be present in multiple namespaces). 
#[derive(Default)]
pub struct CrateNamespace {
    /// The list of all the crates in this namespace,
    /// stored as a map in which the crate's String name
    /// is the key that maps to the value, a strong reference to a crate.
    /// It is a strong reference because a crate must not be removed
    /// as long as it is part of any namespace,
    /// and a single crate can be part of multiple namespaces at once.
    /// For example, the "core" (Rust core library) crate is essentially
    /// part of every single namespace, simply because most other crates rely upon it. 
    crate_tree: Mutex<BTreeMap<String, StrongCrateRef>>,

    /// The "system map" of all global (publicly-visible) symbols
    /// that are present in all of the crates in this `CrateNamespace`.
    /// Maps a fully-qualified symbol name string to a corresponding `LoadedSection`,
    /// which is guaranteed to be part of one of the crates in this `CrateNamespace`.  
    /// Symbols declared as "no_mangle" will appear  root namespace with no crate prefixex, as expected.
    symbol_map: Mutex<SymbolMap>,
}

impl CrateNamespace {
    
    /// Simple debugging function for outputting the symbol map.
    pub fn dump_symbol_map(&self) -> String {
        use core::fmt::Write;
        let mut output: String = String::new();
        let sysmap = self.symbol_map.lock();
        match write!(&mut output, "{:?}", sysmap.keys().collect::<Vec<&String>>()) {
            Ok(_) => output,
            _ => String::from("error"),
        }
    }


    /// Adds a new crate to the module tree, and adds only its global symbols to the system map.
    /// Returns the number of *new* unique global symbols added to the system map. 
    /// If a symbol already exists in the system map, this leaves it intact and *does not* replace it.
    pub fn add_crate(&self, new_crate: StrongCrateRef, log_replacements: bool) -> usize {
        let crate_name = new_crate.read().crate_name.clone();
        let mut system_map = self.symbol_map.lock();
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

        self.crate_tree.lock().insert(crate_name, new_crate);
        count
    }


    /// A convenince function that returns a weak reference to the `LoadedSection`
    /// that matches the given name (`demangled_full_symbol`), if it exists in the system map.
    fn get_symbol_internal(&self, demangled_full_symbol: &str) -> Option<WeakSectionRef> {
        self.symbol_map.lock().get(demangled_full_symbol).cloned()
    }


    /// Finds the corresponding `LoadedSection` reference for the given fully-qualified symbol string.
    /// 
    /// # Note
    /// This is not an interrupt-safe function. DO NOT call it from within an interrupt handler context.
    pub fn get_symbol(&self, demangled_full_symbol: &str) -> WeakSectionRef {
        self.get_symbol_internal(demangled_full_symbol)
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
    pub fn get_symbol_or_load(&self, demangled_full_symbol: &str, kernel_mmi: &mut MemoryManagementInfo) -> WeakSectionRef {
        if let Some(sec) = self.get_symbol_internal(demangled_full_symbol) {
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
                    if let Some(sec) = self.get_symbol_internal(demangled_full_symbol) {
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
}


/// Crate names must be only alphanumeric characters, an underscore, or a dash. 
/// See: <https://www.reddit.com/r/rust/comments/4rlom7/what_characters_are_allowed_in_a_crate_name/>
fn is_valid_crate_name_char(c: char) -> bool {
    char::is_alphanumeric(c) || 
    c == '_' || 
    c == '-'
}
