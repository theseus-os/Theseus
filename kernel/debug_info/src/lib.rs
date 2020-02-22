//! Support for DWARF debug information from ELF files.
//! 
//! This is a good intro to the DWARF format:
//! <http://www.dwarfstd.org/doc/Debugging%20using%20DWARF.pdf>

#![no_std]

extern crate alloc;
#[macro_use] extern crate log;
extern crate gimli;
extern crate xmas_elf;
extern crate goblin;
extern crate util;
extern crate memory;
extern crate fs_node;
extern crate owning_ref;
extern crate crate_metadata;
extern crate mod_mgmt;
extern crate hashbrown;
extern crate by_address;
extern crate rustc_demangle;

use core::ops::{
    DerefMut,
    Range,
};
use alloc::{
    string::{String},
    sync::Arc,
};
use fs_node::WeakFileRef;
use owning_ref::ArcRef;
use memory::{MappedPages, VirtualAddress, MmiRef, FRAME_ALLOCATOR, allocate_pages_by_bytes, EntryFlags};
use xmas_elf::{
    ElfFile,
    sections::{SectionData, SectionData::Rela64, ShType},
};
use goblin::elf::reloc::*;
use gimli::{
    DebugAbbrevOffset,
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
        Reader,
        Section,
    },
};
use rustc_demangle::demangle;
use hashbrown::HashSet;
use by_address::ByAddress;
use crate_metadata::{StrongCrateRef, StrongSectionRef, RelocationEntry, write_relocation};
use mod_mgmt::{CrateNamespace, find_symbol_table};


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
    /// The crate that these debug sections correspond to, which must already be loaded.
    loaded_crate: StrongCrateRef,
    /// The list of sections in foreign crates that these debug sections depend on.
    /// 
    /// Unlike the dependencies list maintained in `LoadedSection`'s `sections_i_depend_on`,
    /// this only contains references to the sections themselves instead of both the section
    /// and the original relocation data (see the `StrongDependency` type),
    /// since this only serves to ensure that these sections are not dropped 
    /// while this debug section exists (thus preserving memory safety),
    /// and not for swapping purposes. 
    _dependencies: HashSet<ByAddress<StrongSectionRef>>,
    /// The file that this debug information was processed from. 
    /// This is useful for reclaiming this debug info's underlying memory
    /// and returning it back into an `Unloaded` state.
    original_file: WeakFileRef,
}
impl DebugSections {
    /// Returns the `".debug_str"` section.
    pub fn debug_str(&self) -> DebugStr<EndianSlice<NativeEndian>> {
        DebugStr::new(&self.debug_str.0, NativeEndian)
    }

    /// Returns the `".debug_loc"` section.
    pub fn debug_loc(&self) -> DebugLoc<EndianSlice<NativeEndian>> {
        DebugLoc::new(&self.debug_loc.0, NativeEndian)
    }

    /// Returns the `".debug_abbrev"` section.
    pub fn debug_abbrev(&self) -> DebugAbbrev<EndianSlice<NativeEndian>> {
        DebugAbbrev::new(&self.debug_abbrev.0, NativeEndian)
    }

    /// Returns the `".debug_info"` section.
    pub fn debug_info(&self) -> DebugInfo<EndianSlice<NativeEndian>> {
        DebugInfo::new(&self.debug_info.0, NativeEndian)
    }

    /// Returns the `".debug_ranges"` section.
    pub fn debug_ranges(&self) -> DebugRanges<EndianSlice<NativeEndian>> {
        DebugRanges::new(&self.debug_ranges.0, NativeEndian)
    }

    /// Returns the `".debug_pubnames"` section.
    pub fn debug_pubnames(&self) -> DebugPubNames<EndianSlice<NativeEndian>> {
        DebugPubNames::new(&self.debug_pubnames.0, NativeEndian)
    }

    /// Returns the `".debug_pubtypes"` section.
    pub fn debug_pubtypes(&self) -> DebugPubTypes<EndianSlice<NativeEndian>> {
        DebugPubTypes::new(&self.debug_pubtypes.0, NativeEndian)
    }

    /// Returns the `".debug_line"` section.
    pub fn debug_line(&self) -> DebugLine<EndianSlice<NativeEndian>> {
        DebugLine::new(&self.debug_line.0, NativeEndian)
    }

    
    /// internal function for recursively traversing a tree of DIE nodes
    /// 
    /// A *lexical block* is DWARF's term for a lexical scope block, e.g., curly braces like so:
    /// ```rust,no_run
    /// fn main() { // start
    ///     let a = 5;
    ///     { // start
    ///         let b = 6
    ///         { // start
    ///             let c = 10;
    ///         } // end
    ///     } // end
    ///     { // start
    ///         let d = 8;
    ///     } // end
    /// } //end
    /// ```
    ///
    /// Otherwise, an error is returned upon failure, e.g., a problem parsing the debug sections.
    fn handle_node<R: Reader>(
        &self,
        instruction_pointer: VirtualAddress,
        depth: usize,
        node: gimli::EntriesTreeNode<R>,
        debug_str_sec: &DebugStr<R>
    ) -> gimli::Result<Option<gimli::UnitOffset<R::Offset>>> {
        let entry = node.entry();
        let tag = entry.tag();
        debug!("{:indent$}DIE code: {:?}, tag: {:?}", "", entry.code(), tag.static_string(), indent = ((depth) * 2));

        // We found a subprogram node.
        // We only care about subprogram nodes that contain the given instruction pointer.
        if tag == gimli::constants::DW_TAG_subprogram {
            let _subprogram_name = entry.attr(gimli::DW_AT_name)?.expect("Subprogram DIE didn't have a DW_AT_name attribute")
                .string_value(debug_str_sec).expect("Couldn't convert subprogram name attribute value to string")
                .to_string().map(|s| String::from(s))?;
            let _subprogram_linkage_name = entry.attr(gimli::DW_AT_linkage_name)?.expect("Subprogram DIE didn't have a DW_AT_linkage_name attribute")
                .string_value(debug_str_sec).expect("Couldn't convert subprogram linkage_name attribute value to string")
                .to_string().map(|s| String::from(s))?;

            let starting_vaddr = match entry.attr_value(gimli::DW_AT_low_pc)? {
                Some(gimli::AttributeValue::Addr(a)) => VirtualAddress::new_canonical(a as usize),
                Some(unsupported) => panic!("unsupported AttributeValue type for low_pc: {:?}", unsupported),
                _ => {
                    debug!("{:indent$}--Subprogram {:?}({:?}) did not have attribute DW_AT_low_pc", "", _subprogram_name, _subprogram_linkage_name, indent = (depth * 2));
                    return Ok(None);
                }
            };
            let size_of_subprogram = match entry.attr_value(gimli::DW_AT_high_pc)? {
                Some(gimli::AttributeValue::Udata(d)) => d as usize,
                Some(unsupported) => panic!("unsupported AttributeValue type for high_pc: {:?}", unsupported),
                _ => {
                    debug!("{:indent$}--Subprogram {:?}({:?}) did not have attribute DW_AT_high_pc", "", _subprogram_name, _subprogram_linkage_name, indent = (depth * 2));
                    return Ok(None);
                }
            };
            let ending_vaddr = starting_vaddr + size_of_subprogram;

            debug!("{:indent$}--Subprogram {:?}({:?}) ranges from {:#X} to {:#X} (size {:#X} bytes)", "",
                _subprogram_name, _subprogram_linkage_name, starting_vaddr, ending_vaddr, size_of_subprogram,
                indent = (depth * 2)
            );

            if instruction_pointer >= starting_vaddr && instruction_pointer < ending_vaddr {
                warn!("{:indent$}--Found matching subprogram at {:?}", "", entry.offset(), indent = ((depth) * 2));
                // Here we found a subprogram that contains the given instruction pointer. 
                // We use a return statement here because once we've found a matching subprogram,
                // we can stop looking at other subprograms because only one subprogram can possibly contain a given instruction pointer.
                return self.handle_node(instruction_pointer, depth + 1, node, debug_str_sec);
            } 
        } 

        // We found a lexical block node. 
        // We only care about lexical block nodes because they may contain variable nodes.
        else if tag == gimli::DW_TAG_lexical_block {
            let low_pc  = entry.attr_value(gimli::DW_AT_low_pc)? .expect("Lexical Block DIE didn't have a DW_AT_low_pc attribute");
            let starting_vaddr = match low_pc {
                gimli::AttributeValue::Addr(a) => VirtualAddress::new_canonical(a as usize),
                unsupported => panic!("unsupported AttributeValue type for low_pc: {:?}", unsupported),
            };
            let high_pc = entry.attr_value(gimli::DW_AT_high_pc)?.expect("Lexical Block DIE didn't have a DW_AT_high_pc attribute");
            let size_of_lexical_block = match high_pc {
                gimli::AttributeValue::Udata(d) => d as usize,
                unsupported => panic!("unsupported AttributeValue type for high_pc: {:?}", unsupported),
            };
            let ending_vaddr = starting_vaddr + size_of_lexical_block;

            debug!("{:indent$}--Lexical Block ranges from {:#X} to {:#X} (size {:#X} bytes)", "",
                starting_vaddr, ending_vaddr, size_of_lexical_block,
                indent = ((depth) * 2)
            );
            
            // Previously, I erroneously thought that we should look for lexical blocks that contain that instr ptr
            // in order to drill down into the precise block that's relevant. 
            // But lexical blocks don't work that way. 
            //
            // Instead, we actually need to look at the **variables** within lexical blocks,
            // and look at the location lists (which describe the range(s) of addresses for which that variable is used, and what register it occupies).
            // If any of *those* ranges contain the given instruction pointer, 
            // then we know that that variable existed and was in-scope at the point that execution reached the instruction pointer.
            // Those are the variables that we want to drop. 

            // // TODO: FIXME: change this according to the above comment block
            // if instruction_pointer >= starting_vaddr && instruction_pointer < ending_vaddr {
            //     warn!("{:indent$}--Found matching lexical block at {:?}", "", entry.offset(), indent = ((depth) * 2));
            //     return self.handle_lexical_block(instruction_pointer, depth + 1, node, debug_str_sec);
            // }   

            // We don't use a "return" statement here because we want to fall through 
            // to the end of this function so we can recurse into the child nodes.
        }

        // We found a variable entry. 
        // Here, we need to look at the location lists (which describe the range(s) of addresses for which that variable is used, and what register it occupies).
        // If any of *those* ranges contain the given instruction pointer, 
        // then we know that that variable existed and was in-scope at the point that execution reached the instruction pointer.
        // Those are the variables that we want to drop. 
        else if tag == gimli::DW_TAG_variable {
            let variable_name = entry.attr(gimli::DW_AT_name)?.expect("Variable DIE didn't have a DW_AT_name attribute")
                .string_value(debug_str_sec).expect("Couldn't convert variable name attribute value to string")
                .to_string().map(|s| String::from(s))?;
            let type_signature = match entry.attr_value(gimli::DW_AT_type)? {
                Some(gimli::AttributeValue::DebugTypesRef(type_ref)) => type_ref,
                unexpected => panic!("unexpected DW_AT_type attribute value: {:?}", unexpected),
            };
            debug!("{:indent$}Variable {:?}, type: {:?}", "", variable_name, type_signature, indent = ((depth) * 2));
            if let Some(loc) = entry.attr(gimli::DW_AT_location)? {
                warn!("{:indent$}Variable {:?}, type: {:?} NMAY NEED HANDLING FOR LOCATION {:?}", "", variable_name, type_signature, loc, indent = ((depth+1) * 2));
            } else {
                // If a variable doesn't have a location, that means it was optimized out and doesn't actually exist in the object code. 
                // So, do nothing here.
            }
            
            // TODO FIXME: check if one of the variable's location ranges contains the instruction pointer
            
            let _debug_pubtypes_sec = self.debug_pubtypes();
            // let type_ref = {
            //     let mut types_iter = debug_pubtypes_sec.items(); 
            //     while let Some(item) = types_iter.next()? {
            //         item
            //     }
            //     panic!("")
            // };
        }

        else { }

        // In all other cases, we simply recurse through the child nodes.

        // Dump the entry's attributes.
        let mut attribute_iter = node.entry().attrs();
        while let Some(attr) = attribute_iter.next()? {
            debug!("{:indent$}Attribute: {:?}, value: {:?}", "", attr.name().static_string(), attr.value(), indent = ((depth + 1) * 2));
            if let Some(s) = attr.string_value(debug_str_sec) {
                trace!("{:indent$}--> Value: {:?}", "", s.to_string(), indent = ((depth + 2) * 2));
            } else {
                trace!("{:indent$}--> Value: None", "", indent = ((depth + 2) * 2));
            }
        }

        // Recurse into the entry node's children nodes.
        let mut children = node.children();
        while let Some(child_subtree) = children.next()? {
            if let Some(offset) = self.handle_node(instruction_pointer, depth + 1, child_subtree, debug_str_sec)? {
                return Ok(Some(offset));
            }
        }

        
        // Didn't find any matching subprogram DIE
        Ok(None)
    }


    /// Finds the subprogram that contains the given instruction pointer. 
    /// 
    /// A *subprogram* is DWARF's term for an executable function/method/closure/subroutine,
    /// which has a bounded range of program counters / instruction pointers that can be searched. 
    /// 
    /// # Return
    /// Returns the offset into the `DebugInfo` of the Debugging Information Entry (DIE) that describes the subprogram
    /// that covers (includes) the virtual address of the given `instruction_pointer`.
    /// 
    /// If a matching subprogram DIE is not found, `Ok(None)` is returned.
    /// 
    /// Otherwise, an error is returned upon failure, e.g., a problem parsing the debug sections.
    pub fn find_subprogram_containing(&self, instruction_pointer: VirtualAddress) -> gimli::Result<Option<gimli::DebugInfoOffset>> {

        warn!("TARGET INSTRUCTION POINTER: {:#X}", instruction_pointer);

        // actual code starts here
        let debug_info_sec = self.debug_info();
        let debug_abbrev_sec = self.debug_abbrev();
        let debug_str_sec = self.debug_str();

        let mut units = debug_info_sec.units();
        // just dump all units 
        while let Some(u) = units.next()? {
            debug!("Unit: {:?}", u);
        }

        // In most cases, there is just one unit. But we go through all of them just in case. 
        let mut units = debug_info_sec.units();
        while let Some(u) = units.next()? {
            let abbreviations = u.abbreviations(&debug_abbrev_sec)?;
            let mut entries_tree = u.entries_tree(&abbreviations, None)?;
            let node = entries_tree.root()?;
            if let Some(offset) = self.handle_node(instruction_pointer, 0, node, &debug_str_sec)? {
                return Ok(Some(offset.to_debug_info_offset(&u)));
            }
        }

        Ok(None)
    }
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
    /// Loads the debug symbols from the enclosed weak file reference
    /// that correspond to the given `LoadedCrate` and using symbols from the given `CrateNamespace`. 
    /// 
    /// If these `DebugSymbols` are already loaded, this is a no-op and simply returns those loaded `DebugSections`.
    pub fn load(&mut self, loaded_crate: &StrongCrateRef, namespace: &CrateNamespace) -> Result<&DebugSections, &'static str> {
        let weak_file = match self {
            Self::Loaded(ds) => return Ok(ds),
            Self::Unloaded(wf) => wf,
        };
        let kernel_mmi_ref = memory::get_kernel_mmi_ref().ok_or("couldn't get kernel MMI")?;
        let file_ref = weak_file.upgrade().ok_or("No debug symbol file found")?;
        let file = file_ref.lock();
        let file_bytes: &[u8] = file.as_mapping()?.as_slice(0, file.size())?;
        let elf_file = ElfFile::new(file_bytes)?;
        let symtab = find_symbol_table(&elf_file)?;

        // Allocate a memory region large enough to hold all debug sections.
        let (mut debug_sections_mp, _debug_sections_vaddr_range) = allocate_debug_section_pages(&elf_file, &kernel_mmi_ref)?;
        debug!("debug sections spans {:#X} to {:#X}  (size: {:#X} bytes)",
            _debug_sections_vaddr_range.start, 
            _debug_sections_vaddr_range.end,
            _debug_sections_vaddr_range.end.value() - _debug_sections_vaddr_range.start.value(),
        );
        let mut mp_offset = 0;

        let mut debug_str:       Option<DebugSection> = None;
        let mut debug_loc:       Option<DebugSection> = None;
        let mut debug_abbrev:    Option<DebugSection> = None;
        let mut debug_info:      Option<DebugSection> = None;
        let mut debug_ranges:    Option<DebugSection> = None;
        let mut debug_pubnames:  Option<DebugSection> = None;
        let mut debug_pubtypes:  Option<DebugSection> = None;
        let mut debug_line:      Option<DebugSection> = None;
        let mut dependencies: HashSet<ByAddress<StrongSectionRef>> = HashSet::new();
        
        for (shndx, sec) in elf_file.section_iter().enumerate() {
            let size = sec.size() as usize;
            let virt_addr = debug_sections_mp.start_address() + mp_offset;
            let dest_slice: &mut [u8] = debug_sections_mp.as_slice_mut(mp_offset, size)?;
            let sec_name = sec.get_name(&elf_file);
            
            if Ok(SectionId::DebugStr.name()) == sec_name {
                debug_str = Some(DebugSection { shndx, virt_addr, mp_offset, size, });
            }
            else if Ok(SectionId::DebugLoc.name()) == sec_name {
                debug_loc = Some(DebugSection { shndx, virt_addr, mp_offset, size, });
            }
            else if Ok(SectionId::DebugAbbrev.name()) == sec_name {
                debug_abbrev = Some(DebugSection { shndx, virt_addr, mp_offset, size, });
            }
            else if Ok(SectionId::DebugInfo.name()) == sec_name {
                debug_info = Some(DebugSection { shndx, virt_addr, mp_offset, size, });
            }
            else if Ok(SectionId::DebugRanges.name()) == sec_name {
                debug_ranges = Some(DebugSection { shndx, virt_addr, mp_offset, size, });
            }
            else if Ok(SectionId::DebugPubNames.name()) == sec_name {
                debug_pubnames = Some(DebugSection { shndx, virt_addr, mp_offset, size, });
            }
            else if Ok(SectionId::DebugPubTypes.name()) == sec_name {
                debug_pubtypes = Some(DebugSection { shndx, virt_addr, mp_offset, size, });
            }
            else if Ok(SectionId::DebugLine.name()) == sec_name {
                debug_line = Some(DebugSection { shndx, virt_addr, mp_offset, size, });
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

        // Ensure we found all of the expected debug sections.
        let debug_str      = debug_str.ok_or("couldn't find .debug_str section")?;
        let debug_loc      = debug_loc.ok_or("couldn't find .debug_loc section")?;
        let debug_abbrev   = debug_abbrev.ok_or("couldn't find .debug_abbrev section")?;
        let debug_info     = debug_info.ok_or("couldn't find .debug_info section")?;
        let debug_ranges   = debug_ranges.ok_or("couldn't find .debug_ranges section")?;
        let debug_pubnames = debug_pubnames.ok_or("couldn't find .debug_pubnames section")?;
        let debug_pubtypes = debug_pubtypes.ok_or("couldn't find .debug_pubtypes section")?;
        let debug_line     = debug_line.ok_or("couldn't find .debug_line section")?;

        if true {
            debug!("Section .debug_str loaded from {:#X} to {:#X} (size {:#X} bytes)", debug_str.virt_addr, debug_str.virt_addr + debug_str.size, debug_str.size);
            debug!("Section .debug_loc loaded from {:#X} to {:#X} (size {:#X} bytes)", debug_loc.virt_addr, debug_loc.virt_addr + debug_loc.size, debug_loc.size);
            debug!("Section .debug_abbrev loaded from {:#X} to {:#X} (size {:#X} bytes)", debug_abbrev.virt_addr, debug_abbrev.virt_addr + debug_abbrev.size, debug_abbrev.size);
            debug!("Section .debug_info loaded from {:#X} to {:#X} (size {:#X} bytes)", debug_info.virt_addr, debug_info.virt_addr + debug_info.size, debug_info.size);
            debug!("Section .debug_ranges loaded from {:#X} to {:#X} (size {:#X} bytes)", debug_ranges.virt_addr, debug_ranges.virt_addr + debug_ranges.size, debug_ranges.size);
            debug!("Section .debug_pubnames loaded from {:#X} to {:#X} (size {:#X} bytes)", debug_pubnames.virt_addr, debug_pubnames.virt_addr + debug_pubnames.size, debug_pubnames.size);
            debug!("Section .debug_pubtypes loaded from {:#X} to {:#X} (size {:#X} bytes)", debug_pubtypes.virt_addr, debug_pubtypes.virt_addr + debug_pubtypes.size, debug_pubtypes.size);
            debug!("Section .debug_line loaded from {:#X} to {:#X} (size {:#X} bytes)", debug_line.virt_addr, debug_line.virt_addr + debug_line.size, debug_line.size);
        }

        let shndx_map = [
            (debug_str.shndx, &debug_str),
            (debug_loc.shndx, &debug_loc),
            (debug_abbrev.shndx, &debug_abbrev),
            (debug_info.shndx, &debug_info),
            (debug_ranges.shndx, &debug_ranges),
            (debug_pubnames.shndx, &debug_pubnames),
            (debug_pubtypes.shndx, &debug_pubtypes),
            (debug_line.shndx, &debug_line),
        ];

        // A convenience function that searches for the local debug section with the given `shndx`
        let find_debug_section_by_shndx = |shndx: &usize| -> Option<&DebugSection> {
            shndx_map.iter()
                .find(|(ndx, _)| shndx == ndx)
                .map(|&(_, sec)| sec)
        };

        // Now that we've loaded the debug sections into memory, we can perform the relocations for those sections. 
        for sec in elf_file.section_iter().filter(|sec| sec.get_type() == Ok(ShType::Rela)) {
            // The target section is where we write the relocation data to.
            // The source section is where we get the data from (typically just its virtual address). 

            // The "info" field in this `sec` specifies the shndx of the target section.
            let target_sec_shndx = sec.info() as usize;
            let target_sec = match find_debug_section_by_shndx(&target_sec_shndx) {
                Some(sec) => sec,
                _ => continue,
            };
            
            // There is one target section per rela section (`rela_array`), and one source section per rela_entry in this rela section.
            let rela_array = match sec.get_data(&elf_file) {
                Ok(Rela64(rela_arr)) => rela_arr,
                _ => {
                    error!("Found Rela section that wasn't able to be parsed as Rela64: {:?}", sec);
                    return Err("Found Rela section that wasn't able to be parsed as Rela64");
                } 
            };
            
            // iterate through each relocation entry in the relocation array for the target_sec
            for rela_entry in rela_array {
                if false { 
                    trace!("      Rela64 offset: {:#X}, addend: {:#X}, symtab_index: {}, type: {:#X}", 
                        rela_entry.get_offset(), rela_entry.get_addend(), rela_entry.get_symbol_table_index(), rela_entry.get_type());
                }

                use xmas_elf::symbol_table::Entry;
                let source_sec_entry = &symtab[rela_entry.get_symbol_table_index() as usize];
                let source_sec_shndx = source_sec_entry.shndx() as usize; 
                if false { 
                    let source_sec_header_name = source_sec_entry.get_section_header(&elf_file, rela_entry.get_symbol_table_index() as usize)
                        .and_then(|s| s.get_name(&elf_file));
                    trace!("             relevant section [{}]: {:?}", source_sec_shndx, source_sec_header_name);
                    // trace!("             Entry name {} {:?} vis {:?} bind {:?} type {:?} shndx {} value {} size {}", 
                    //     source_sec_entry.name(), source_sec_entry.get_name(&elf_file), 
                    //     source_sec_entry.get_other(), source_sec_entry.get_binding(), source_sec_entry.get_type(), 
                    //     source_sec_entry.shndx(), source_sec_entry.value(), source_sec_entry.size());
                }
                
                let mut source_and_target_in_same_crate = false;

                // We first check if the source section is another debug section, then check if its a local section from the given `loaded_crate`.
                let (source_sec_vaddr, source_sec_dep) = match find_debug_section_by_shndx(&source_sec_shndx).map(|s| (s.virt_addr, None))
                    .or_else(|| loaded_crate.lock_as_ref().sections.get(&source_sec_shndx).map(|sec| (sec.address_range.start, Some(sec.clone()))))
                {
                    // We found the source section in the local debug sections or the given loaded crate. 
                    Some(found) => {
                        source_and_target_in_same_crate = true;
                        Ok(found)
                    }

                    // If we couldn't get the source section based on its shndx, it means that the source section was in a foreign crate.
                    // Thus, we must get the source section's name and check our list of foreign crates to see if it's there.
                    // At this point, there's no other way to search for the source section besides its name.
                    None => {
                        if let Ok(source_sec_name) = source_sec_entry.get_name(&elf_file) {
                            const DATARELRO: &'static str = ".data.rel.ro.";
                            let source_sec_name = if source_sec_name.starts_with(DATARELRO) {
                                source_sec_name.get(DATARELRO.len() ..).ok_or("Couldn't get name of .data.rel.ro. section")?
                            } else {
                                source_sec_name
                            };
                            use alloc::string::ToString;
                            let demangled = demangle(source_sec_name).to_string();
                            warn!("Looking for foreign relocation source section {:?}", demangled);

                            // search for the symbol's demangled name in the kernel's symbol map
                            namespace.get_symbol_or_load(&demangled, None, &kernel_mmi_ref, false)
                                .upgrade()
                                .ok_or("Couldn't get symbol for .debug section's foreign relocation entry, nor load its containing crate")
                                .map(|sec| (sec.address_range.start, Some(sec)))
                        }
                        else {
                            let _source_sec_header = source_sec_entry
                                .get_section_header(&elf_file, rela_entry.get_symbol_table_index() as usize)
                                .and_then(|s| s.get_name(&elf_file));
                            error!("Couldn't get name of source section [{}] {:?}, needed for non-local relocation entry", source_sec_shndx, _source_sec_header);
                            Err("Couldn't get source section's name, needed for non-local relocation entry")
                        }
                    }
                }?;
                
                let relocation_entry = RelocationEntry::from_elf_relocation(rela_entry);
                write_relocation_debug(
                    relocation_entry,
                    &mut debug_sections_mp,
                    target_sec.mp_offset,
                    source_sec_vaddr,
                    false
                )?;

                // If these debug sections have a dependency on a section in a foreign crate, 
                // add that dependency here to prevent that foreign crate's section from being dropped while we still depend on it.
                if !source_and_target_in_same_crate {
                    warn!("Found foreign dependency on source section {:?}", source_sec_dep);
                    if let Some(ss) = source_sec_dep {
                        dependencies.insert(ByAddress(Arc::clone(&ss)));
                    }
                }
            } // end of relocations for a given target section
        } // end of all relocations


        // The .debug sections were initially mapped as writable so we could modify them,
        // but they should actually just be read-only as specified by the ELF file flags.
        debug_sections_mp.remap(&mut kernel_mmi_ref.lock().page_table, EntryFlags::PRESENT)?; 
        let debug_sections_mp = Arc::new(debug_sections_mp);

        let create_debug_section_slice = |debug_sec: DebugSection| -> Result<DebugSectionSlice, &'static str> {
            ArcRef::new(Arc::clone(&debug_sections_mp))
                .try_map(|mp| mp.as_slice::<u8>(debug_sec.mp_offset, debug_sec.size))
                .map(|arcref| DebugSectionSlice(arcref))
        };

        let loaded = DebugSections {
            debug_str:       create_debug_section_slice(debug_str)?,
            debug_loc:       create_debug_section_slice(debug_loc)?,
            debug_abbrev:    create_debug_section_slice(debug_abbrev)?,
            debug_info:      create_debug_section_slice(debug_info)?,
            debug_ranges:    create_debug_section_slice(debug_ranges)?,
            debug_pubnames:  create_debug_section_slice(debug_pubnames)?,
            debug_pubtypes:  create_debug_section_slice(debug_pubtypes)?,
            debug_line:      create_debug_section_slice(debug_line)?,
            loaded_crate:    loaded_crate.clone_shallow(),
            _dependencies:   dependencies,
            original_file:   weak_file.clone(),
        };
        *self = Self::Loaded(loaded);
        match self {
            Self::Loaded(d) => Ok(d), 
            Self::Unloaded(_) => Err("BUG: unreachable: debug sections were loaded but DebugSymbols enum was wrong type"),
        }
    }

    /// A convenience method for accessing the already-loaded `DebugSections` within.
    /// Returns `None` if the symbols are not currently loaded.
    pub fn get_loaded(&self) -> Option<&DebugSections> {
        match self {
            Self::Loaded(d) => Some(d), 
            Self::Unloaded(_) => None,
        }
    }

    /// Unloads these `DebugSymbols`, returning the enclosed `DebugSections` if they were already loaded.
    /// If not, this is a no-op and returns `None`.
    /// 
    /// This is useful to free the large memory regions needed for debug information,
    /// and also to release dependencies on other crates' sections.  
    pub fn unload(&mut self) -> Option<DebugSections>{
        let weak_file = match self {
            Self::Unloaded(_) => return None,
            Self::Loaded(ds) => ds.original_file.clone(),
        };
        let old = core::mem::replace(self, Self::Unloaded(weak_file));
        match old {
            Self::Loaded(d) => Some(d), 
            Self::Unloaded(_) => None, // unreachable
        }
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

        // trace!("  Looking at debug sec {:?}, size {:#X}, align {:#X} --> addend {:#X}", sec.get_name(elf_file), size, align, addend);
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

/// An internal struct used to store metadata about a debug section
/// while it is still being linked. 
/// 
/// This is used to perform relocations on debug sections before they can be used.
/// The final form of a ready-to-use debug section should be a `DebugSectionSlice`, not this type. 
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

/// A slice that contains the exact byte range of fully-linked debug section.
struct DebugSectionSlice(ArcRef<MappedPages, [u8]>);




/// Write a relocation entry for a `.debug_*` section. 
/// 
/// # Implementation and Usage Note
/// I'm not entirely sure if this is implemented correctly, since it differs from the regular relocation formulas. 
/// However, it seems to work with Gimli. 
/// It may be that Gimli itself is broken in the way that it uses offsets:
/// gimli seems to expect just an offset value to be written at the relocation target address, 
/// rather than a direct (full, non-offset) value.
/// 
/// # Arguments
/// * `relocation_entry`: the relocation entry from the ELF file that specifies the details of the relocation action to perform.
/// * `target_sec_mapped_pages`: the `MappedPages` that covers the target section, i.e., the section where the relocation data will be written to.
/// * `target_sec_mapped_pages_offset`: the offset into `target_sec_mapped_pages` where the target section is located.
/// * `source_sec_vaddr`: the `VirtualAddress` of the source section of the relocation, i.e., the section that the `target_sec` depends on and "points" to.
/// * `verbose_log`: whether to output verbose logging information about this relocation action.
fn write_relocation_debug(
    relocation_entry: RelocationEntry,
    target_sec_mapped_pages: &mut MappedPages,
    target_sec_mapped_pages_offset: usize,
    source_sec_vaddr: VirtualAddress,
    verbose_log: bool
) -> Result<(), &'static str> {
    match relocation_entry.typ {
        R_X86_64_32 => {
            // Calculate exactly where we should write the relocation data to.
            let target_offset = target_sec_mapped_pages_offset + relocation_entry.offset;
            let target_ref: &mut u32 = target_sec_mapped_pages.as_type_mut(target_offset)?;
            // For this relocation entry type, typically we would use "target = source + addend".
            // But for debug sections, apparently we just want to use "target = addend".
            let source_val = relocation_entry.addend;
            if verbose_log { trace!("                    target_ptr: {:#X}, source_val: {:#X} (ignoring source_sec_vaddr {:#X})", target_ref as *mut _ as usize, source_val, source_sec_vaddr); }
            *target_ref = source_val as u32;
            Ok(())
        }
        _ => {
            // Otherwise, we use the standard relocation formulas.
            write_relocation(relocation_entry, target_sec_mapped_pages, target_sec_mapped_pages_offset, source_sec_vaddr, verbose_log)
        }
    }
}