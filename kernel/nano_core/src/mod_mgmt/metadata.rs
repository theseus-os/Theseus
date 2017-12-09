use alloc::{Vec, String};
use memory::VirtualAddress;


pub struct LoadedCrate {
    text_sections: Vec<LoadedTextSection> ,
    crate_dependencies: Vec<LoadedCrate>,
}


/// Represents a module's .text section, which in Rust,
/// corresponds to a single function. 
pub struct LoadedTextSection {
    /// The full String representation of this symbol. 
    /// Format <crate>::<module>::<struct>::<fn_name>
    /// For example, test_lib::MyStruct::new
    abs_symbol: String,
    /// The virtual address of where this text section is loaded
    virt_addr: VirtualAddress,
    /// The size in bytes of this section
    size: usize,
}

