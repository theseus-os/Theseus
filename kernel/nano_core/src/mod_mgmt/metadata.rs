use spin::Mutex;
use alloc::{Vec, String, BTreeMap};
use alloc::arc::{Arc, Weak};
use memory::VirtualAddress;
use memory::virtual_address_allocator::OwnedContiguousPages;


lazy_static! {
    /// The main metadata structure that contains a tree of all loaded crates.
    /// Maps a String crate_name to its crate instance.
    static ref CRATE_TREE: Mutex<BTreeMap<String, LoadedCrate>> = Mutex::new(BTreeMap::new());
}


lazy_static! {
    /// A flat map of all symbols currently loaded into the kernel. 
    /// Maps a fully-qualified kernel symbol name (String) to the corresponding `LoadedSection`. 
    /// Symbols declared as "no_mangle" will appear in the root namespace with no crate prefixex, as expected.
    static ref SYSTEM_MAP: Mutex<BTreeMap<String, Weak<LoadedSection>>> = Mutex::new(BTreeMap::new());
}


/// simple debugging function
pub fn dump_symbol_map() -> String {
    use core::fmt::Write;
    let mut output: String = String::new();
    match write!(&mut output, "{:?}", *SYSTEM_MAP.lock()) {
        Ok(_) => output,
        _ => String::from("error"),
    }
}


/// Adds a new crate to the module tree, and adds its symbols to the system map. 
pub fn add_crate(new_crate: LoadedCrate) {
    
    // add all the global symbols to the system map
    {
        let mut locked_kmap = SYSTEM_MAP.lock();
        for sec in new_crate.sections.iter().filter(|s| s.is_global()) {
            if let Some(key) = sec.key() {
                let old_val = locked_kmap.insert(key.clone(), Arc::downgrade(sec));
                // as of now we don't expect/support replacing a symbol (section) in the system map
                if old_val.is_some() {
                    warn!("Unexpected: replacing existing entry in system map: {} -> {:?}", key, old_val);
                }
            }
        }
    }
    CRATE_TREE.lock().insert(new_crate.crate_name.clone(), new_crate);
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
    pub owned_pages: Vec<OwnedContiguousPages>,
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
            &LoadedSection::Rodata(ref rodata) => None,
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
    pub fn set_global(&mut self, is_global: bool) {
        match self {
            &mut LoadedSection::Text(ref mut text) => text.global = is_global,
            &mut LoadedSection::Rodata(ref mut rodata) => rodata.global = is_global,
            &mut LoadedSection::Data(ref mut data) => data.global = is_global,
        }
    }
}


/// Represents a .text section in a crate, which in Rust,
/// corresponds to a single function. 
#[derive(Debug)]
pub struct TextSection {
    /// The String representation of just this symbol,
    /// without any preceding crate namespaces or anything.
    pub symbol: String,
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
    /// The String representation of just this symbol,
    /// without any preceding crate namespaces or anything.
    pub symbol: String,
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
    /// The String representation of just this symbol,
    /// without any preceding crate namespaces or anything.
    pub symbol: String,
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