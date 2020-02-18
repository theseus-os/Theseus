//! Support for DWARF debug information from ELF files.
//! 

#![no_std]

extern crate alloc;
#[macro_use] extern crate log;
extern crate gimli;
extern crate xmas_elf;
extern crate util;
extern crate memory;
extern crate fs_node;
extern crate owning_ref;
extern crate crate_metadata;
extern crate hashbrown;
extern crate by_address;

use core::ops::{
    DerefMut,
    Range,
};
use alloc::sync::Arc;
use fs_node::WeakFileRef;
use owning_ref::ArcRef;
use memory::{MappedPages, VirtualAddress, MmiRef, FRAME_ALLOCATOR, allocate_pages_by_bytes, EntryFlags};
use xmas_elf::{
    ElfFile,
    sections::SectionData,
};
use gimli::{
    NativeEndian,
    EndianSlice,
    SectionId,
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
use hashbrown::HashSet;
use by_address::ByAddress;
use crate_metadata::StrongSectionRef;


/// The set of debug sections that we need to use from a crate object file.
/// 
/// All debug sections herein are contained within a single `MappedPages` memory region.
pub struct DebugSections {
    debug_str:       DebugSectionSlice,
    debug_loc:       DebugSectionSlice,
    debug_abbrev:    DebugSectionSlice,
    debug_info:      DebugSectionSlice,
    debug_ranges:    DebugSectionSlice,
    debug_pubnames:  DebugSectionSlice,
    debug_pubtypes:  DebugSectionSlice,
    debug_line:      DebugSectionSlice,
    /// The list of sections in foreign crates that these debug sections depend on.
    /// 
    /// Unlike the dependencies list maintained in `LoadedSection`'s `sections_i_depend_on`,
    /// this only contains references to the sections themselves instead of both the section
    /// and the original relocation data (see the `StrongDependency` type),
    /// since this only serves to ensure that these sections are not dropped 
    /// while this debug section exists (thus preserving memory safety),
    /// and not for swapping purposes. 
    dependencies: HashSet<ByAddress<StrongSectionRef>>,
    /// The file that this debug information was processed from. 
    /// This is useful for reclaiming this debug info's underlying memory
    /// and returning it back into an `Unloaded` state.
    original_file: WeakFileRef,
}
impl DebugSections {
    /// The `".debug_str"` section.
    pub fn debug_str(&self) -> DebugStr<EndianSlice<NativeEndian>> {
        DebugStr::new(&self.debug_str.0, NativeEndian)
    }

    // /// The `".debug_loc"` section.
    // pub debug_loc:       DebugLoc<      EndianSlice<'i, NativeEndian>>,     
    // /// The `".debug_abbrev"` section.
    // pub debug_abbrev:    DebugAbbrev<   EndianSlice<'i, NativeEndian>>,
    // /// The `".debug_info"` section.
    // pub debug_info:      DebugInfo<     EndianSlice<'i, NativeEndian>>,
    // /// The `".debug_ranges"` section.
    // pub debug_ranges:    DebugRanges<   EndianSlice<'i, NativeEndian>>,
    // /// The `".debug_pubnames"` section.
    // pub debug_pubnames:  DebugPubNames< EndianSlice<'i, NativeEndian>>,
    // /// The `".debug_pubtypes"` section.
    // pub debug_pubtypes:  DebugPubTypes< EndianSlice<'i, NativeEndian>>,
    // /// The `".debug_line"` section.
    // pub debug_line:      DebugLine<     EndianSlice<'i, NativeEndian>>,

}

/// An enum describing the possible forms of debug information for a crate. 
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
        let kernel_mmi_ref = memory::get_kernel_mmi_ref().ok_or("couldn't get kernel MMI")?;
        let file_ref = weak_file.upgrade().ok_or("No debug symbol file found")?;
        let file = file_ref.lock();
        let file_bytes: &[u8] = file.as_mapping()?.as_slice(0, file.size())?;
        let elf_file = ElfFile::new(file_bytes)?;

        // Allocate a memory region large enough to hold all debug sections.
        let (mut debug_sections_mp, debug_sections_vaddr_range) = allocate_debug_section_pages(&elf_file, &kernel_mmi_ref)?;
        let mut mp_offset = 0;

        let mut debug_str_bounds:       Option<DebugSection> = None;
        let mut debug_loc_bounds:       Option<DebugSection> = None;
        let mut debug_abbrev_bounds:    Option<DebugSection> = None;
        let mut debug_info_bounds:      Option<DebugSection> = None;
        let mut debug_ranges_bounds:    Option<DebugSection> = None;
        let mut debug_pubnames_bounds:  Option<DebugSection> = None;
        let mut debug_pubtypes_bounds:  Option<DebugSection> = None;
        let mut debug_line_bounds:      Option<DebugSection> = None;
        let mut dependencies: HashSet<ByAddress<StrongSectionRef>> = HashSet::new();
        
        for (shndx, sec) in elf_file.section_iter().enumerate() {
            let size = sec.size() as usize;
            let virt_addr = debug_sections_mp.start_address() + mp_offset;
            let dest_slice: &mut [u8] = debug_sections_mp.as_slice_mut(mp_offset, size)?;
            let sec_name = sec.get_name(&elf_file);
            
            if Ok(SectionId::DebugStr.name()) == sec_name {
                debug_str_bounds = Some(DebugSection { shndx, virt_addr, mp_offset, size, });
            }
            else if Ok(SectionId::DebugLoc.name()) == sec_name {
                debug_loc_bounds = Some(DebugSection { shndx, virt_addr, mp_offset, size, });
            }
            else if Ok(SectionId::DebugAbbrev.name()) == sec_name {
                debug_abbrev_bounds = Some(DebugSection { shndx, virt_addr, mp_offset, size, });
            }
            else if Ok(SectionId::DebugInfo.name()) == sec_name {
                debug_info_bounds = Some(DebugSection { shndx, virt_addr, mp_offset, size, });
            }
            else if Ok(SectionId::DebugRanges.name()) == sec_name {
                debug_ranges_bounds = Some(DebugSection { shndx, virt_addr, mp_offset, size, });
            }
            else if Ok(SectionId::DebugPubNames.name()) == sec_name {
                debug_pubnames_bounds = Some(DebugSection { shndx, virt_addr, mp_offset, size, });
            }
            else if Ok(SectionId::DebugPubTypes.name()) == sec_name {
                debug_pubtypes_bounds = Some(DebugSection { shndx, virt_addr, mp_offset, size, });
            }
            else if Ok(SectionId::DebugLine.name()) == sec_name {
                debug_line_bounds = Some(DebugSection { shndx, virt_addr, mp_offset, size, });
            }
            else {
                continue;
            }
            
            // Copy this debug section's content from the ELF file into the previously-allocated memory region.
            match sec.get_data(&elf_file) {
                Ok(SectionData::Undefined(sec_data)) => dest_slice.copy_from_slice(sec_data),
                _ => {
                    error!("couldn't get section data for {:?}: {:?}", sec_name, sec.get_data(&elf_file));
                    return Err("couldn't get section data for .debug_* section section");
                }
            }

            mp_offset += size;
        }


        // TODO: we need to perform relocations here

        



        // The .debug sections were initially mapped as writable so we could modify them,
        // but they should actually just be read-only as specified by the ELF file flags.
        debug_sections_mp.remap(&mut kernel_mmi_ref.lock().page_table, EntryFlags::PRESENT)?; 
        let debug_sections_mp = Arc::new(debug_sections_mp);

        let create_debug_section_slice = |bounds: Option<DebugSection>, err_msg: &'static str| -> Result<DebugSectionSlice, &'static str> {
            bounds.ok_or(err_msg)
                .and_then(|b| ArcRef::new(Arc::clone(&debug_sections_mp)).try_map(|mp| mp.as_slice::<u8>(b.mp_offset, b.size)))
                .map(|arcref| DebugSectionSlice(arcref))
        };

        let loaded = DebugSections {
            debug_str:       create_debug_section_slice(debug_str_bounds,      "couldn't find .debug_str section")?,
            debug_loc:       create_debug_section_slice(debug_loc_bounds,      "couldn't find .debug_loc section")?,
            debug_abbrev:    create_debug_section_slice(debug_abbrev_bounds,   "couldn't find .debug_abbrev section")?,
            debug_info:      create_debug_section_slice(debug_info_bounds,     "couldn't find .debug_info section")?,
            debug_ranges:    create_debug_section_slice(debug_ranges_bounds,   "couldn't find .debug_ranges section")?,
            debug_pubnames:  create_debug_section_slice(debug_pubnames_bounds, "couldn't find .debug_pubnames section")?,
            debug_pubtypes:  create_debug_section_slice(debug_pubtypes_bounds, "couldn't find .debug_pubtypes section")?,
            debug_line:      create_debug_section_slice(debug_line_bounds,     "couldn't find .debug_line section")?,
            dependencies:    dependencies,
            original_file:   weak_file.clone(),
        };
        *self = Self::Loaded(loaded);
        match self {
            Self::Loaded(d) => Ok(d), 
            Self::Unloaded(_) => unreachable!(),
        }
    }

    pub fn unload(&mut self) {
        let weak_file = match self {
            Self::Unloaded(_) => return,
            Self::Loaded(ds) => ds.original_file.clone(),
        };
        *self = Self::Unloaded(weak_file);
        // upon return, the loaded `DebugSections` struct will be dropped
    }

    pub fn dump_structs(&mut self, /*_krate: &LoadedCrate*/) -> Result<(), &'static str> {
        Err("Unfinished")
    }
}


/// Allocates and maps memory sufficient to hold the `".debug_*` sections that are found in the given `ElfFile`.
/// 
/// This function can be refactored and combined with `mod_mgmt::allocate_section_pages()`.
fn allocate_debug_section_pages(elf_file: &ElfFile, kernel_mmi_ref: &MmiRef) -> Result<(MappedPages, Range<VirtualAddress>), &'static str> {
    let mut ro_bytes = 0;
    for sec in elf_file.section_iter() {
        // Skip non-"debug" sections.
        if sec.get_name(elf_file).map(|n| n.starts_with(".debug_")) != Ok(true) {
            continue;
        }

        let size = sec.size() as usize;
        let align = sec.align() as usize;
        let addend = util::round_up_power_of_two(size, align);

        trace!("  Looking at debug sec {:?}, size {:#X}, align {:#X} --> addend {:#X}", sec.get_name(elf_file), size, align, addend);
        ro_bytes += addend;
    }

    if ro_bytes == 0 {
        return Err("no .debug sections found");
    }

    let mut frame_allocator = FRAME_ALLOCATOR.try().ok_or("couldn't get FRAME_ALLOCATOR")?.lock();
    let allocated_pages = allocate_pages_by_bytes(ro_bytes).ok_or("Couldn't allocate_pages_by_bytes, out of virtual address space")?;
    let mp = kernel_mmi_ref.lock().page_table.map_allocated_pages(allocated_pages, EntryFlags::PRESENT | EntryFlags::WRITABLE, frame_allocator.deref_mut())?;
    let start_address = mp.start_address();
    let range = start_address .. (start_address + ro_bytes);
    Ok((mp, range))
}


struct DebugSection {
    // /// The type of this debug section.
    // id: SectionId,
    /// The section header index in the ELF file for this section.
    shndx: usize,
    /// The starting `VirtualAddress` of this section,
    /// primarily a performance optimization used for handling relocations.
    virt_addr: VirtualAddress,
    /// The offset into the `MappedPages` where this section starts.
    /// That `MappedPages` object contains all debug sections.
    mp_offset: usize,
    /// The size in bytes of this section.
    size: usize,
}

struct DebugSectionSlice(ArcRef<MappedPages, [u8]>);
