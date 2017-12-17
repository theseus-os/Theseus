use spin::Mutex;
use alloc::{Vec, String, BTreeMap};
use alloc::arc::Arc;
use memory::VirtualAddress;
use memory::virtual_address_allocator::OwnedContiguousPages;
// use concurrent_hashmap::ConcHashMap;

lazy_static! {
    /// A flat map of all symbols currently loaded into the kernel. 
    /// Maps a fully-qualified kernel symbol name (String) to its VirtualAddress. 
    // static ref KERNEL_SYMBOL_TABLE: Arc<ConcHashMap<String, VirtualAddress>> = 
    //            Arc::new(ConcHashMap::new());
    static ref KERNEL_SYMBOL_TABLE: Mutex<BTreeMap<String, VirtualAddress>> = 
            Mutex::new(BTreeMap::new());
}

#[derive(Debug)]
pub struct LoadedCrate {
    pub crate_name: String,
    pub sections: Vec<LoadedSection>,
    pub owned_pages: Vec<OwnedContiguousPages>,
    // crate_dependencies: Vec<LoadedCrate>,
}


#[derive(Debug)]
pub enum LoadedSection{
    Text(TextSection),
    Rodata(RodataSection), // TODO: add type
    Data(DataSection),  // TODO: add type
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
    /// the unique hash generated for this function by the Rust compiler,
    /// which can be used as a version identifier. 
    /// Not all symbols will have a hash, like those that are not mangled.
    pub hash: Option<String>,
    /// The virtual address of where this text section is loaded
    pub virt_addr: VirtualAddress,
    /// The size in bytes of this section
    pub size: usize,
}


/// Represents a .rodata section in a crate.
#[derive(Debug)]
pub struct RodataSection {
    /// The virtual address of where this section is loaded
    pub virt_addr: VirtualAddress,
    /// The size in bytes of this section
    pub size: usize,
}


/// Represents a .data section in a crate.
#[derive(Debug)]
pub struct DataSection {
    /// The virtual address of where this section is loaded
    pub virt_addr: VirtualAddress,
    /// The size in bytes of this section
    pub size: usize,
}