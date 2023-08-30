//! Standalone crate containing (de)serializable types for crate and section metadata.
//! 
//! The primary reason this exists is because `LoadedCrate` and `LoadedSection`
//! make copious usage of `Arc` and `Weak` reference-counted pointer types,
//! which cannot be properly (de)serialized by `serde`. 
//! The types in this module remove those refcount types so that they can be
//! reconstructed individually at runtime after deserialization by the `crate_metadata` crate.
//!
//! This is currently only used to parse and serialize the `nano_core` binary at compile time.
//! The `nano_core`'s [`SerializedCrate`] is then included as a boot module
//! so it can be deserialized into a LoadedCrate at runtime by `mod_mgmt`.
//! 
//! Some other types have been moved from `crate_metadata` into this crate because
//! they are required for (de)serialization, e.g., [`SectionType`].
//! 
//! ## Goal: minimal dependencies
//! This crate's dependencies should be kept to a bare minimum in order to 
//! minimize the dependencies of the `tools/serialize_nano_core` executable,
//! allowing it to build and run quickly.
//! 
//! Thus, instead of using Theseus-specific types here in this crate,
//! we prefer using types from this crate in other Theseus kernel crates.
//! For example, instead of implementing the routines to convert a `SerializedCrate`
//! into a `LoadedCrate` here, we implement the routine to create a `LoadedCrate`
//! from a `SerializedCrate` in the `mod_mgmt` crate itself.
//! In other words, other larger/complex Theseus crates should depend on this crate
//! instead of this crate depending on other Theseus crates.

#![no_std]

extern crate alloc;

use alloc::{
    collections::{BTreeMap, BTreeSet},
    string::String,
};
use hashbrown::HashMap;
use serde::{Deserialize, Serialize};

/// The flag identifying CLS sections.
pub const CLS_SECTION_FLAG: u64 = 0x100000;

/// The type identifying CLS symbols.
pub const CLS_SYMBOL_TYPE: u8 = 0xa;

/// A (de)serializable representation of a loaded crate that is `serde`-compatible.
///
/// See `LoadedCrate` for more detail on the fields of this struct.
#[derive(Debug, Serialize, Deserialize)]
pub struct SerializedCrate {
    /// The name of the crate.
    pub crate_name: String,
    /// A map containing all the sections of the crate.
    pub sections: HashMap<Shndx, SerializedSection>,
    /// A set containing the global sections of the crate.
    pub global_sections: BTreeSet<Shndx>,
    /// A set containing the thread-local storage (TLS) sections of the crate.
    pub tls_sections: BTreeSet<Shndx>,
    /// The CLS section of the crate.
    pub cls_sections: BTreeSet<Shndx>,
    /// A set containing the `.data` and `.bss` sections of the crate.
    pub data_sections: BTreeSet<Shndx>,
    /// A map of symbol names to their constant values, which contain assembler and linker
    /// constants.
    pub init_symbols: BTreeMap<String, usize>,
}

/// A (de)serializable representation of a loaded section that is `serde`-compatible.
///
/// See `LoadedSection` for more detail on the fields of this struct.
#[derive(Debug, Serialize, Deserialize)]
pub struct SerializedSection {
    /// The full name of the section.
    pub name: String,
    /// The type of the section.
    pub ty: SectionType,
    /// Whether or not the section is global.
    pub global: bool,
    /// The starting virtual address of the range covered by this section.
    pub virtual_address: usize,
    /// The offset into this section's containing `MappedPages` where this section starts.
    pub offset: usize,
    /// The size of the section.
    pub size: usize,
}

/// A Section Header iNDeX (SHNDX), as specified by the ELF format. 
/// Even though this is typically encoded as a `u16`,
/// its decoded form can exceed the max size of `u16`.
pub type Shndx = usize;

pub const TEXT_SECTION_NAME             : &str = ".text";
pub const RODATA_SECTION_NAME           : &str = ".rodata";
pub const DATA_SECTION_NAME             : &str = ".data";
pub const BSS_SECTION_NAME              : &str = ".bss";
pub const TLS_DATA_SECTION_NAME         : &str = ".tdata";
pub const TLS_BSS_SECTION_NAME          : &str = ".tbss";
pub const CLS_SECTION_NAME              : &str = ".cls";
pub const GCC_EXCEPT_TABLE_SECTION_NAME : &str = ".gcc_except_table";
pub const EH_FRAME_SECTION_NAME         : &str = ".eh_frame";

/// The possible types of sections that can be loaded from a crate object file.
#[derive(Debug, Copy, Clone, PartialEq, Serialize, Deserialize)]
pub enum SectionType {
    /// A `text` section contains executable code, i.e., functions. 
    Text,
    /// An `rodata` section contains read-only data, i.e., constants.
    Rodata,
    /// A `data` section contains data that is both readable and writable, i.e., static variables. 
    Data,
    /// A `bss` section is just like a data section, but is automatically initialized to all zeroes at load time.
    Bss,
    /// A `.tdata` section is a read-only section that holds the initial data "image" 
    /// for a thread-local storage (TLS) area.
    TlsData,
    /// A `.tbss` section is a read-only section that holds all-zero data for a thread-local storage (TLS) area.
    /// This is is effectively an empty placeholder: the all-zero data section doesn't actually exist in memory.
    TlsBss,
    /// A `.cls` section is a read-only section that holds the initial data "image" for a CPU-local
    /// (CLS) area.
    Cls,
    /// A `.gcc_except_table` section contains landing pads for exception handling,
    /// comprising the LSDA (Language Specific Data Area),
    /// which is effectively used to determine when we should stop the stack unwinding process
    /// (e.g., "catching" an exception). 
    /// 
    /// Blog post from author of gold linker: <https://www.airs.com/blog/archives/464>
    /// 
    /// Mailing list discussion here: <https://gcc.gnu.org/ml/gcc-help/2010-09/msg00116.html>
    /// 
    /// Here is a sample repository parsing this section: <https://github.com/nest-leonlee/gcc_except_table>
    /// 
    GccExceptTable,
    /// The `.eh_frame` section contains information about stack unwinding and destructor functions
    /// that should be called when traversing up the stack for cleanup. 
    /// 
    /// Blog post from author of gold linker: <https://www.airs.com/blog/archives/460>
    /// Some documentation here: <https://gcc.gnu.org/wiki/Dwarf2EHNewbiesHowto>
    /// 
    EhFrame,
}
impl SectionType {    
    /// Returns the const `&str` name of this `SectionType`.
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Text           => TEXT_SECTION_NAME,
            Self::Rodata         => RODATA_SECTION_NAME,
            Self::Data           => DATA_SECTION_NAME, 
            Self::Bss            => BSS_SECTION_NAME,
            Self::TlsData        => TLS_DATA_SECTION_NAME,
            Self::TlsBss         => TLS_BSS_SECTION_NAME,
            Self::Cls            => CLS_SECTION_NAME,
            Self::GccExceptTable => GCC_EXCEPT_TABLE_SECTION_NAME,
            Self::EhFrame        => EH_FRAME_SECTION_NAME,
        }
    } 

    /// Returns `true` if `Data` or `Bss`, otherwise `false`.
    pub fn is_data_or_bss(&self) -> bool {
        matches!(self, Self::Data | Self::Bss)
    }

    /// Returns `true` if `TlsData` or `TlsBss`, otherwise `false`.
    pub fn is_tls(&self) -> bool {
        matches!(self, Self::TlsData | Self::TlsBss)
    }
}
