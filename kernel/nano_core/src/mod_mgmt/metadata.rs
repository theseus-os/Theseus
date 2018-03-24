use spin::Mutex;
use irq_safety::MutexIrqSafe;
use alloc::{Vec, String, BTreeMap};
use alloc::arc::{Arc, Weak};
use alloc::btree_map::Entry; 
use memory::{VirtualAddress, MappedPages};

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
        let crate_name = new_crate.crate_name.clone();
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
                                if log_replacements { info!("       Crate \"{}\": Ignoring new symbol already present: {}", crate_name, key); }
                            }
                            else {
                                warn!("       Unexpected: crate \"{}\": different section sizes (old={}, new={}) when ignoring new symbol in system map: {}", 
                                    crate_name, old_sec.size(), new_sec_size, key);
                            }
                        }
                    }
                    Entry::Vacant(new) => {
                        new.insert(Arc::downgrade(sec));
                    }
                }

                
                // BELOW: the old way that just blindly replaced the old symbol with the new
                // let old_val = locked_kmap.insert(key.clone(), Arc::downgrade(sec));
                // debug!("Crate \"{}\": added new symbol: {} at vaddr: {:#X}", crate_name, key, sec.virt_addr());
                // if let Some(old_sec) = old_val.and_then(|w| w.upgrade()) {
                //     if old_sec.size() == new_sec_size {
                //         if true || log_replacements { info!("       Crate \"{}\": Replaced existing entry in system map: {}", crate_name, key); }
                //     }
                //     else {
                //         warn!("       Unexpected: crate \"{}\": different section sizes (old={}, new={}) when replacing existing entry in system map: {}", 
                //                crate_name, old_sec.size(), new_sec_size, key);
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
    pub crate_name: String,
    pub sections: Vec<Arc<LoadedSection>>,
    pub mapped_pages: Vec<MappedPages>,
    // crate_dependencies: Vec<LoadedCrate>,
}



#[derive(Debug)]
pub enum LoadedSection{
    Text(TextSection),
    Rodata(RodataSection),
    Data(DataSection),
}
impl LoadedSection {
    pub fn virt_addr(&self) -> VirtualAddress {
        match self {
            &LoadedSection::Text(ref text) => text.virt_addr,
            &LoadedSection::Rodata(ref rodata) => rodata.virt_addr,
            &LoadedSection::Data(ref data) => data.virt_addr,
        }
    }
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
    // pub fn set_global(&mut self, is_global: bool) {
    //     match self {
    //         &mut LoadedSection::Text(ref mut text) => text.global = is_global,
    //         &mut LoadedSection::Rodata(ref mut rodata) => rodata.global = is_global,
    //         &mut LoadedSection::Data(ref mut data) => data.global = is_global,
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
    /// The virtual address of where this text section is loaded
    pub virt_addr: VirtualAddress,
    /// The size in bytes of this section
    pub size: usize,
    /// Whether or not this section's symbol was exported globally (is public)
    pub global: bool,
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
    /// The virtual address of where this section is loaded
    pub virt_addr: VirtualAddress,
    /// The size in bytes of this section
    pub size: usize,
    /// Whether or not this section's symbol was exported globally (is public)
    pub global: bool,
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
    /// The virtual address of where this section is loaded
    pub virt_addr: VirtualAddress,
    /// The size in bytes of this section
    pub size: usize,
    /// Whether or not this section's symbol was exported globally (is public)
    pub global: bool,
}