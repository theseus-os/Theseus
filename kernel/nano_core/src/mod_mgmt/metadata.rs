use alloc::{Vec, String};
use memory::VirtualAddress;
use memory::virtual_address_allocator::OwnedPages;

pub struct LoadedCrate {
    pub crate_name: String,
    pub text_sections: Vec<LoadedTextSection>,
    pub owned_pages: OwnedPages,
    // crate_dependencies: Vec<LoadedCrate>,
}


/// Represents a module's .text section, which in Rust,
/// corresponds to a single function. 
pub struct LoadedTextSection {
    /// The String representation of just this symbol,
    /// without any 
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

