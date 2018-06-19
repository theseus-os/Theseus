use alloc::string::String;
use rustc_demangle::demangle;


/// A representation of a demangled symbol.
/// # Example
/// mangled:            "_ZN7console4init17h71243d883671cb51E"
/// demangled.no_hash:  "console::init"
/// demangled.hash:     "h71243d883671cb51E"
pub struct DemangledSymbol {
    pub no_hash: String, 
    pub hash: Option<String>,
}

pub fn demangle_symbol(s: &str) -> DemangledSymbol {
    let demangled = demangle(s);
    let without_hash: String = format!("{:#}", demangled); // the fully-qualified symbol, no hash
    let with_hash: String = format!("{}", demangled); // the fully-qualified symbol, with the hash
    let hash_only: Option<String> = with_hash.find::<&str>(without_hash.as_ref())
        .and_then(|index| {
            let hash_start = index + 2 + without_hash.len();
            with_hash.get(hash_start..).map(|s| String::from(s))
        }); // + 2 to skip the "::" separator
    
    DemangledSymbol {
        no_hash: without_hash,
        hash: hash_only,
    }
}