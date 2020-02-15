//! Support for DWARF debug information from ELF files.
//! 

#![no_std]

extern crate alloc;
extern crate gimli;
extern crate memory;
extern crate fs_node;
extern crate owning_ref;

use fs_node::WeakFileRef;
use owning_ref::ArcRef;
use memory::MappedPages;

use gimli::{
    NativeEndian,
    EndianSlice,
    read::{
        DebugAbbrev,
        DebugInfo,
        DebugLine,
        DebugLoc,
        DebugRanges,
        DebugPubNames,
        DebugPubTypes,
        DebugStr,
    },
};


// pub struct DebugSections<'i> {
//     pub debug_str:       DebugStr<      EndianSlice<'i, NativeEndian>>,
//     pub debug_loc:       DebugLoc<      EndianSlice<'i, NativeEndian>>,     
//     pub debug_abbrev:    DebugAbbrev<   EndianSlice<'i, NativeEndian>>,
//     pub debug_info:      DebugInfo<     EndianSlice<'i, NativeEndian>>,
//     pub debug_ranges:    DebugRanges<   EndianSlice<'i, NativeEndian>>,
//     pub debug_pubnames:  DebugPubNames< EndianSlice<'i, NativeEndian>>,
//     pub debug_pubtypes:  DebugPubTypes< EndianSlice<'i, NativeEndian>>,
//     pub debug_line:      DebugLine<     EndianSlice<'i, NativeEndian>>,
// }


/// A placeholder struct that will contain debug information for a crate.
#[derive(Clone)]
pub struct DebugSections {
    /// The read-only memory region that holds all debug sections.
    mapped_pages: ArcRef<MappedPages, [u8]>,
    /// The file that this debug information was processed from. 
    /// This is useful for reclaiming this debug info's underlying memory
    /// and returning it back into an `Unloaded` state.
    original_file: WeakFileRef,
}

/// An enum describing the possible forms of debug information for a crate. 
#[derive(Clone)]
pub enum DebugSymbols {
    /// Debug information that hasn't yet been parsed from the given file. 
    /// We use a weak reference to the file because it's not mandatory to have debug symbols.
    Unloaded(WeakFileRef),
    /// The debug information has already been parsed from the file
    Loaded(DebugSections),
}
impl DebugSymbols {
    pub fn load(&mut self) -> Result<&DebugSections, &'static str> {
        let weak_file = match self {
            Self::Loaded(ds) => return Ok(ds),
            Self::Unloaded(wf) => wf,
        };
        let file_ref = weak_file.upgrade().ok_or("No debug symbol file found")?;

        Err("unfinished")
    }

    pub fn dump_structs(&mut self, /*_krate: &LoadedCrate*/) -> Result<(), &'static str> {
        Err("Unfinished")
    }
}