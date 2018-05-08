//! This metadata module contains metadata about all other modules/crates loaded in Theseus.
//! 
//! [This is a good link](https://users.rust-lang.org/t/circular-reference-issue/9097)
//! for understanding why we need `Arc`/`Weak` to handle recursive/circular data structures in Rust. 

use spin::Mutex;
use irq_safety::MutexIrqSafe;
use alloc::{Vec, String, BTreeMap};
use alloc::arc::{Arc, Weak};
use alloc::btree_map::Entry; 
use memory::MappedPages;

lazy_static! {
    /// The main metadata structure that contains a tree of all loaded crates.
    /// Maps a String crate_name to its crate instance.
    static ref CRATE_TREE: Mutex<BTreeMap<String, LoadedCrate>> = Mutex::new(BTreeMap::new());
}


lazy_static! {
    /// A flat map of all symbols currently loaded into the kernel. 
    /// Maps a fully-qualified kernel symbol name (String) to the corresponding `LoadedSection`. 
    /// Symbols declared as "no_mangle" will appear in the root namespace with no crate prefixex, as expected.
    /// Currently we use a MutexIrqSafe because the metadata can be queried via `get_symbol` from an interrupt context.
    /// Later, when the nano_core is fully minimalized, we should be able to remove this. 
    /// TODO FIXME: change this MutexIrqSafe back to a regular Mutex once we stop using `get_symbol` in IRQ contexts. 
    static ref SYSTEM_MAP: MutexIrqSafe<BTreeMap<String, Weak<LoadedSection>>> = MutexIrqSafe::new(BTreeMap::new());
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


/// Adds a new crate to the module tree, and adds its symbols to the system map.
/// Returns the number of global symbols added to the system map. 
pub fn add_crate(new_crate: LoadedCrate, log_replacements: bool) -> usize {
    let mut count = 0;
    // add all the global symbols to the system map
    {
        let mut locked_kmap = SYSTEM_MAP.lock();
        for sec in new_crate.sections.iter().filter(|s| s.is_global()) {
            let new_sec_size = sec.size();

            if let Some(key) = sec.key() {
                // instead of blindly replacing old symbols with their new version, we leave all old versions intact 
                // TODO NOT SURE IF THIS IS THE CORRECT WAY, but blindly replacing them all is definitely wrong
                let entry = locked_kmap.entry(key.clone());
                match entry {
                    Entry::Occupied(old_val) => {
                        if let Some(old_sec) = old_val.get().upgrade() {
                            if old_sec.size() == new_sec_size {
                                if log_replacements { info!("       Crate \"{}\": Ignoring new symbol already present: {}", new_crate.crate_name, key); }
                            }
                            else {
                                warn!("       Unexpected: crate \"{}\": different section sizes (old={}, new={}) when ignoring new symbol in system map: {}", 
                                    new_crate.crate_name, old_sec.size(), new_sec_size, key);
                            }
                        }
                    }
                    Entry::Vacant(new) => {
                        new.insert(Arc::downgrade(sec));
                    }
                }

                
                // BELOW: the old way that just blindly replaced the old symbol with the new
                // let old_val = locked_kmap.insert(key.clone(), Arc::downgrade(sec));
                // debug!("Crate \"{}\": added new symbol: {} at vaddr: {:#X}", new_crate.crate_name, key, sec.virt_addr());
                // if let Some(old_sec) = old_val.and_then(|w| w.upgrade()) {
                //     if old_sec.size() == new_sec_size {
                //         if true || log_replacements { info!("       Crate \"{}\": Replaced existing entry in system map: {}", crate_name, key); }
                //     }
                //     else {
                //         warn!("       Unexpected: crate \"{}\": different section sizes (old={}, new={}) when replacing existing entry in system map: {}", 
                //                new_crate.crate_name, old_sec.size(), new_sec_size, key);
                //     }
                // }

                count += 1;
                // debug!("add_crate(): [{}], new symbol: {}", new_crate.crate_name, key);
            }
        }
    }
    CRATE_TREE.lock().insert(new_crate.crate_name.clone(), new_crate);
    count
}


/// Finds the corresponding `LoadedSection` reference for the given fully-qualified symbol String.
pub fn get_symbol<S: Into<String>>(symbol: S) -> Weak<LoadedSection> {
    match SYSTEM_MAP.lock().get(&symbol.into()) {
        Some(sec) => sec.clone(),
        _ => Weak::default(),
    }
}




#[derive(Debug)]
pub struct LoadedCrate {
    /// The name of this crate
    pub crate_name: String,
    /// The list of all sections in this crate.
    pub sections: Vec<Arc<LoadedSection>>,
    /// The `MappedPages` that include the text sections for this crate,
    /// i.e., sections that are readable and executable.
    pub text_pages: Option<Arc<MappedPages>>,
    /// The `MappedPages` that include the rodata sections for this crate.
    /// i.e., sections that are read-only, not writable nor executable.
    pub rodata_pages: Option<Arc<MappedPages>>,
    /// The `MappedPages` that include the data and bss sections for this crate.
    /// i.e., sections that are readable and writable but not executable.
    pub data_pages: Option<Arc<MappedPages>>,

    // crate_dependencies: Vec<LoadedCrate>,
}



#[derive(Debug)]
pub enum LoadedSection{
    Text(TextSection),
    Rodata(RodataSection),
    Data(DataSection),
}
impl LoadedSection {
    pub fn size(&self) -> usize {
        match self {
            &LoadedSection::Text(ref text) => text.size,
            &LoadedSection::Rodata(ref rodata) => rodata.size,
            &LoadedSection::Data(ref data) => data.size,
        }
    }
    pub fn key(&self) -> Option<String> {
        match self {
            &LoadedSection::Text(ref text) => Some(text.abs_symbol.clone()),
            &LoadedSection::Rodata(ref rodata) => Some(rodata.abs_symbol.clone()),
            &LoadedSection::Data(ref data) => Some(data.abs_symbol.clone()),
        }
    }
    pub fn is_global(&self) -> bool {
        match self {
            &LoadedSection::Text(ref text) => text.global,
            &LoadedSection::Rodata(ref rodata) => rodata.global,
            &LoadedSection::Data(ref data) => data.global,
        }
    }
    pub fn mapped_pages_offset(&self) -> usize {
        match self {
            &LoadedSection::Text(ref text) => text.mapped_pages_offset,
            &LoadedSection::Rodata(ref rodata) => rodata.mapped_pages_offset,
            &LoadedSection::Data(ref data) => data.mapped_pages_offset,
        }
    }
    pub fn mapped_pages(&self) -> Option<Arc<MappedPages>> {
        match self {
            &LoadedSection::Text(ref text) => text.mapped_pages.upgrade(),
            &LoadedSection::Rodata(ref rodata) => rodata.mapped_pages.upgrade(),
            &LoadedSection::Data(ref data) => data.mapped_pages.upgrade(),
        }
    }

    // pub fn parent(&self) -> &String {
    //     match self {
    //         &LoadedSection::Text(ref text) => &text.parent_crate,
    //         &LoadedSection::Rodata(ref rodata) => &rodata.parent_crate,
    //         &LoadedSection::Data(ref data) => &data.parent_crate,
    //     }
    // }
}


/// Represents a .text section in a crate, which in Rust,
/// corresponds to a single function. 
#[derive(Debug)]
pub struct TextSection {
    // /// The String representation of just this symbol,
    // /// without any preceding crate namespaces or anything.
    // pub symbol: String,
    /// The full String representation of this symbol. 
    /// Format <crate>::<module>::<struct>::<fn_name>
    /// For example, test_lib::MyStruct::new
    pub abs_symbol: String,
    /// the unique hash generated for this section by the Rust compiler,
    /// which can be used as a version identifier. 
    /// Not all symbols will have a hash, like those that are not mangled.
    pub hash: Option<String>,
    /// A reference to the `MappedPages` object that covers this section
    pub mapped_pages: Weak<MappedPages>,
    /// The offset into the `MappedPages` where this section starts
    pub mapped_pages_offset: usize,
    /// The size in bytes of this section
    pub size: usize,
    /// Whether or not this section's symbol was exported globally (is public)
    pub global: bool,
    /// The name of the `LoadedCrate` object that contains/owns this section
    pub parent_crate: String,
}


/// Represents a .rodata section in a crate.
#[derive(Debug)]
pub struct RodataSection {
    // /// The String representation of just this symbol,
    // /// without any preceding crate namespaces or anything.
    // pub symbol: String,
    /// The full String representation of this symbol. 
    /// Format <crate>::<module>::<struct>::<fn_name>
    /// For example, test_lib::MyStruct::new
    pub abs_symbol: String,
    /// the unique hash generated for this section by the Rust compiler,
    /// which can be used as a version identifier. 
    /// Not all symbols will have a hash, like those that are not mangled.
    pub hash: Option<String>,
    /// A reference to the `MappedPages` object that covers this section
    pub mapped_pages: Weak<MappedPages>,
    /// The offset into the `MappedPages` where this section starts
    pub mapped_pages_offset: usize,
    /// The size in bytes of this section
    pub size: usize,
    /// Whether or not this section's symbol was exported globally (is public)
    pub global: bool,
    /// The name of the `LoadedCrate` object that contains/owns this section
    pub parent_crate: String,
}


/// Represents a .data section in a crate.
#[derive(Debug)]
pub struct DataSection {
    // /// The String representation of just this symbol,
    // /// without any preceding crate namespaces or anything.
    // pub symbol: String,
    /// The full String representation of this symbol. 
    /// Format <crate>::<module>::<struct>::<fn_name>
    /// For example, test_lib::MyStruct::new
    pub abs_symbol: String,
    /// the unique hash generated for this section by the Rust compiler,
    /// which can be used as a version identifier. 
    /// Not all symbols will have a hash, like those that are not mangled.
    pub hash: Option<String>,
    /// A reference to the `MappedPages` object that covers this section
    pub mapped_pages: Weak<MappedPages>,
    /// The offset into the `MappedPages` where this section starts
    pub mapped_pages_offset: usize,
    /// The size in bytes of this section
    pub size: usize,
    /// Whether or not this section's symbol was exported globally (is public)
    pub global: bool,
    /// The name of the `LoadedCrate` object that contains/owns this section
    pub parent_crate: String,
}