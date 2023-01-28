//! An application that loads C language ELF executables atop Theseus.
//!
//! This will be integrated into the Theseus kernel in the future, 
//! likely as a separate crate that integrates well with the `mod_mgmt` crate.

#![no_std]

extern crate alloc;
#[macro_use] extern crate log;
#[macro_use] extern crate app_io;
extern crate getopts;
extern crate fs_node;
extern crate path;
extern crate memory;
extern crate rustc_demangle;
extern crate mod_mgmt;
extern crate task;
extern crate xmas_elf;
extern crate libc; // for basic C types/typedefs used in libc

use core::{
    cmp::{min, max},
    ops::{AddAssign, SubAssign, Range},
};
use alloc::{collections::BTreeSet, string::{String, ToString}, sync::Arc, vec::Vec};
use getopts::{Matches, Options};
use memory::{Page, MappedPages, VirtualAddress, PteFlagsArch, PteFlags};
use mod_mgmt::{CrateNamespace, StrongDependency, find_symbol_table, RelocationEntry, write_relocation};
use rustc_demangle::demangle;
use path::Path;
use xmas_elf::{
    ElfFile,
    program::SegmentData,
    sections::ShType,
};

pub fn main(args: Vec<String>) -> isize {
    let mut opts = Options::new();
    opts.optflag("h", "help", "print this help menu");

    let matches = match opts.parse(&args) {
        Ok(m) => m,
        Err(_f) => {
            println!("{}", _f);
            print_usage(opts);
            return -1; 
        }
    };

    if matches.opt_present("h") {
        print_usage(opts);
        return 0;
    }

    match rmain(matches) {
        Ok(retval) => retval as isize,
        Err(e) => {
            println!("Error:\n{}", e);
            -1
        }    
    }
}


fn rmain(matches: Matches) -> Result<c_int, String> {
    let (curr_wd, namespace, mmi) = task::with_current_task(|curr_task|
        (
            curr_task.get_env().lock().working_dir.clone(),
            curr_task.get_namespace().clone(),
            curr_task.mmi.clone(),
        )
    ).map_err(|_| String::from("failed to get current task"))?;

    let path = matches.free.get(0).ok_or_else(|| "Missing path to ELF executable".to_string())?;
    let path = Path::new(path.clone());
    let file_ref = path.get_file(&curr_wd)
        .ok_or_else(|| format!("Failed to access file at {path:?}"))?;
    let file = file_ref.lock();

    // Parse the file as an ELF executable
    let file_mp = file.as_mapping().map_err(String::from)?;
    let byte_slice: &[u8] = file_mp.as_slice(0, file.len())?;

    let (mut segments, entry_point, _vaddr_offset, elf_file) = parse_and_load_elf_executable(byte_slice)?;
    debug!("Parsed ELF executable, moving on to overwriting relocations.");
    
    // Now, overwrite (recalculate) the relocations that refer to symbols that already exist in Theseus,
    // most important of which are static data sections, 
    // as it is logically incorrect to have duplicates of data that are supposed to be global system-wide singletons.
    // We should throw a warning here if there are no relocations in the file, as it was probably built/linked with the wrong arguments.
    overwrite_relocations(&namespace, &mut segments, &elf_file, &mmi, false)?;

    // Remap each segment's mapped pages using the correct flags; they were previously mapped as always writable.
    {
        let page_table = &mut mmi.lock().page_table;
        for segment in segments.iter_mut() {
            if segment.mp.flags() != segment.flags {
                segment.mp.remap(page_table, segment.flags)?;
            }
        }
    }

    segments.iter().enumerate().for_each(|(i, seg)| debug!("Segment {} needed {} relocations to be rewritten.", i, seg.sections_i_depend_on.len()) );

    let _executable = LoadedExecutable { segments, entry_point }; // must persist through the entire executable's runtime.
    
    debug!("Jumping to entry point {:#X}", entry_point);

    let dummy_args = ["hello", "world"];
    let dummy_env = ["USER=root", "PWD=/"];

    // TODO: FIXME: use `MappedPages::as_func()` instead of `transmute()`.
    let start_fn: StartFunction = unsafe { core::mem::transmute(entry_point.value()) };
    let c_retval = start_fn(&dummy_args, &dummy_env);

    debug!("C _start entry point returned value {}({:#X})", c_retval, c_retval);

    Ok(c_retval)
}

/// Corresponds to C function:  `int foo()`
use libc::c_int;
type StartFunction = fn(args: &[&str], env: &[&str]) -> c_int;


#[allow(unused)]
struct LoadedExecutable {
    segments: Vec<LoadedSegment>,
    entry_point: VirtualAddress,
}


/// Represents an ELF program segment that has been loaded into memory. 
#[derive(Debug)]
#[allow(dead_code)]
pub struct LoadedSegment {
    /// The memory region allocated to hold this program segment.
    mp: MappedPages,
    /// The specific range of virtual addresses occupied by this 
    /// (may be a subset)
    bounds: Range<VirtualAddress>,
    /// The proper flags for this segment specified by the ELF file.
    flags: PteFlagsArch,
    /// The indices of the sections in the ELF file 
    /// that were grouped ("mapped") into this segment by the linker.
    section_ndxs: BTreeSet<usize>,
    /// The list of sections in existing Theseus crates that this segment's sections depends on,
    /// i.e., the required dependencies that must exist as long as this segment.
    sections_i_depend_on: Vec<StrongDependency>,
}

#[derive(Debug, Copy, Clone)]
enum Offset {
    Positive(usize),
    Negative(usize),
}
impl Offset {
    /// Returns a new `Offset` object that represents the adjustment
    /// needed to go from `first` to `second`.
    fn new(first: usize, second: usize) -> Offset {
        if first < second {
            Offset::Negative(second - first)
        } else {
            Offset::Positive(first - second)
        }
    }

    /// Mutably adjusts the given `obj` by the given `offset`.
    fn adjust_assign<T: AddAssign<usize> + SubAssign<usize>>(obj: &mut T, offset: Offset) {
        match offset {
            Offset::Negative(subtrahend) => *obj -= subtrahend,
            Offset::Positive(addend)     => *obj += addend,
        }
    }
}


/// Parses an elf executable file from the given slice of bytes and load it into memory.
///
/// # Important note about memory mappings
/// This function will allocate new memory regions to store each program segment
/// and copy each segment's data into them.
/// When this function returns, those segments will be mapped as writable in order to allow them 
/// to be modified as needed.
/// Before running this executable, each segment's `MappedPages` should be remapped
/// to the proper `flags` specified in its `LoadedSegment.flags` field. 
///
/// # Return
/// Returns a tuple of:
/// 1. A list of program segments mapped into memory. 
/// 2. The virtual address of the executable's entry point, e.g., the `_start` function.
///    This is the function that we should call to start running the executable.
/// 3. The `Offset` by which all virtual addresses in the loaded executable should be shifted by. 
///    This is the difference between where the program is *actually* loaded in memory
///    and where the program *expected* to be loaded into memory.
/// 4. A reference to the parsed `ElfFile`, whose lifetime is tied to the given `file_contents` parameter.
fn parse_and_load_elf_executable(
    file_contents: &[u8],
) -> Result<(Vec<LoadedSegment>, VirtualAddress, Offset, ElfFile), String> {
    debug!("Parsing Elf executable of size {}", file_contents.len());

    let elf_file = ElfFile::new(file_contents).map_err(String::from)?;

    // check that elf_file is an executable type 
    let typ = elf_file.header.pt2.type_().as_type();
    if typ != xmas_elf::header::Type::Executable {
        error!("parse_elf_executable(): ELF file has wrong type {:?}, must be an Executable Elf File!", typ);
        return Err("not a relocatable elf file".into());
    }

    // Currently we aren't building C programs in a position-independent manner,
    // so we have to load the C executable at the exact virtual address it specifies (since it's non-relocatable).

    // TODO FIXME: remove this old approach of invalidly loading non-PIE executables at other virtual addresses than what they expect.
    //             Also remove the whole idea of the "Offset", since that will be built into position-independent executables.
    //             This is because this only works for SUPER SIMPLE C programs, in which we can just maintain the *relative* position of each segment
    //             in memory with respect to other segments to ensure they're consistent. 
    // 
    // Not really necessary to do this, but we iterate over all segments first to find the total range of virtual pages we must allocate. 
    let (mut start_vaddr, mut end_vaddr) = (usize::MAX, usize::MIN);
    let mut num_segments = 0;
    for prog_hdr in elf_file.program_iter() {
        if prog_hdr.get_type() == Ok(xmas_elf::program::Type::Load) {
            num_segments += 1;
            start_vaddr = min(start_vaddr, prog_hdr.virtual_addr() as usize);
            end_vaddr   = max(end_vaddr,   prog_hdr.virtual_addr() as usize + prog_hdr.mem_size() as usize);
        }
    }

    let mut mapped_segments = Vec::with_capacity(num_segments);

    // Allocate enough virtually-contiguous space for all the segments together.
    let total_size_in_bytes = end_vaddr - start_vaddr;
    let mut all_pages = memory::allocate_pages_by_bytes_at(
        VirtualAddress::new(start_vaddr).ok_or_else(|| format!("Segment had invalid virtual address {start_vaddr:#X}"))?,
        total_size_in_bytes
    ).map_err(|_| format!("Failed to allocate {total_size_in_bytes} bytes at {start_vaddr}"))?;
    let vaddr_adjustment = Offset::new(all_pages.start_address().value(), start_vaddr); 

    // Iterate through each segment again and map them into pages we just allocated above,
    // copying their segment data to the proper location.
    for (segment_ndx, prog_hdr) in elf_file.program_iter().enumerate() {
        // debug!("\nLooking at {}", prog_hdr);
        if prog_hdr.get_type() != Ok(xmas_elf::program::Type::Load) {
            // warn!("Skipping non-LOAD segment {:?}", prog_hdr);
            continue;
        }

        // A segment (program header) has two sizes: 
        // 1) memory size: the size in memory that the segment, when loaded, will actually consume. 
        //    This is how much virtual memory space we have to allocate for it. 
        // 2) file size: the size of the segment's actual data from the ELF file itself. 
        //    This is how much data we will actually copy from the file's segment into our allocated memory.
        // The difference is primarily due to .bss sections, in which the file size will be less than the memory size. 
        // If memory size > file size, the difference should be filled with zeros.
        let memory_size_in_bytes = prog_hdr.mem_size() as usize;
        let file_size_in_bytes = prog_hdr.file_size() as usize;
        if memory_size_in_bytes == 0 {
            // warn!("Skipping zero-sized LOAD segment {:?}", prog_hdr);
            continue; 
        }

        let mut start_vaddr = VirtualAddress::new(prog_hdr.virtual_addr() as usize).ok_or_else(|| {
            error!("Program header virtual address was invalid: {:?}", prog_hdr);
            "Program header had an invalid virtual address"
        })?;
        Offset::adjust_assign(&mut start_vaddr, vaddr_adjustment);
        let end_page = Page::containing_address(start_vaddr + (memory_size_in_bytes - 1));

        // debug!("Splitting {:?} after end page {:?}", all_pages, end_page);

        let (this_ap, remaining_pages) = all_pages.split(end_page + 1).map_err(|_ap|
            format!("Failed to split allocated pages {_ap:?} at page {start_vaddr:#X}")
        )?;
        all_pages = remaining_pages;
        // debug!("Successfully split pages into {:?} and {:?}", this_ap, all_pages);
        // debug!("Adjusted segment vaddr: {:#X}, size: {:#X}, {:?}", start_vaddr, memory_size_in_bytes, this_ap.start_address());

        let initial_flags = convert_to_pte_flags(prog_hdr.flags());
        let mmi = task::with_current_task(|t| t.mmi.clone()).unwrap();
        // Must initially map the memory as writable so we can copy the segment data to it later. 
        let mut mp = mmi.lock().page_table
            .map_allocated_pages(this_ap, initial_flags.writable(true))
            .map_err(String::from)?;

        // Copy data from this section into the correct offset into our newly-mapped pages
        let offset_into_mp = mp.offset_of_address(start_vaddr).ok_or_else(|| 
            format!("BUG: destination address {start_vaddr:#X} wasn't within segment's {mp:?}")
        )?;
        match prog_hdr.get_data(&elf_file).map_err(String::from)? {
            SegmentData::Undefined(segment_data) => {
                // debug!("Segment had undefined data of {} ({:#X}) bytes, file size {} ({:#X})",
                //     segment_data.len(), segment_data.len(), file_size_in_bytes, file_size_in_bytes);
                let dest_slice: &mut [u8] = mp.as_slice_mut(offset_into_mp, memory_size_in_bytes).map_err(String::from)?;
                dest_slice[..file_size_in_bytes].copy_from_slice(&segment_data[..file_size_in_bytes]);
                if memory_size_in_bytes > file_size_in_bytes {
                    // debug!("    Zero-filling extra bytes for segment from range [{}:{}).", file_size_in_bytes, dest_slice.len());
                    dest_slice[file_size_in_bytes..].fill(0);
                }
            }
            other => {
                warn!("Segment had data of unhandled type: {:?}", other);
            }
        };

        let segment_bounds = start_vaddr .. (start_vaddr + memory_size_in_bytes);

        // Populate the set of sections that comprise this segment.
        let mut section_ndxs = BTreeSet::new();
        for (shndx, sec) in elf_file.section_iter().enumerate() {
            if segment_bounds.contains(&VirtualAddress::new_canonical(sec.address() as usize)) {
                section_ndxs.insert(shndx);
            }
        }

        debug!("Loaded segment {} at {:X?} contains sections: {:?}", segment_ndx, segment_bounds, section_ndxs);

        mapped_segments.push(LoadedSegment {
            mp,
            bounds: segment_bounds,
            flags: initial_flags.into(),
            section_ndxs,
            sections_i_depend_on: Vec::new(), // this is populated later in `overwrite_relocations()`
        });
    }

    let entry_point = elf_file.header.pt2.entry_point() as usize;
    let mut entry_point_vaddr = VirtualAddress::new(entry_point)
        .ok_or_else(|| format!("ELF entry point was invalid virtual address: {entry_point:#X}"))?;
    Offset::adjust_assign(&mut entry_point_vaddr, vaddr_adjustment);
    debug!("ELF had entry point {:#X}, adjusted to {:#X}", entry_point, entry_point_vaddr);

    Ok((mapped_segments, entry_point_vaddr, vaddr_adjustment, elf_file))
}



/// This function uses the relocation sections in the given `ElfFile` to 
/// rewrite relocations that depend on source sections already existing and currently loaded in Theseus. 
///
/// This is necessary to ensure that the newly-loaded ELF executable depends on and references 
/// the real singleton instances of each data sections (aka `OBJECT`s in ELF terminology) 
/// rather than using the duplicate instance of those data sections in the executable itself. 
fn overwrite_relocations(
    namespace: &Arc<CrateNamespace>,
    segments: &mut [LoadedSegment],
    elf_file: &ElfFile,
    mmi: &memory::MmiRef,
    verbose_log: bool
) -> Result<(), String> {
    let symtab = find_symbol_table(elf_file)?;

    // Fix up the sections that were just loaded, using proper relocation info.
    // Iterate over every non-zero relocation section in the file
    for sec in elf_file.section_iter().filter(|sec| sec.get_type() == Ok(ShType::Rela) && sec.size() != 0) {
        use xmas_elf::sections::SectionData::Rela64;
        if verbose_log { 
            trace!("Found Rela section name: {:?}, type: {:?}, target_sec_index: {:?}", 
                sec.get_name(elf_file), sec.get_type(), sec.info()
            ); 
        }

        let rela_sec_name = sec.get_name(elf_file).unwrap();
        // Skip debug special sections for now, those can be processed later. 
        if rela_sec_name.starts_with(".rela.debug")  { 
            continue;
        }
        // Skip .eh_frame relocations, since they are all local to the .text section
        // and cannot depend on external symbols directly
        if rela_sec_name == ".rela.eh_frame"  { 
            continue;
        }

        let rela_array = match sec.get_data(elf_file) {
            Ok(Rela64(rela_arr)) => rela_arr,
            _ => {
                let err = format!("Found Rela section that wasn't able to be parsed as Rela64: {sec:?}");
                error!("{}", err);
                return Err(err);
            } 
        };

        // The target section (segment) is where we write the relocation data to.
        // The source section is where we get the data from. 
        // There is one target section per rela section (`rela_array`), and one source section per `rela_entry` in each `rela_array`.
        // The "info" field in the Rela section specifies which section is the target of the relocation.
            
        // Get the target section (that we already loaded) for this rela_array Rela section.
        let target_sec_shndx = sec.info() as usize;
        let target_segment = segments.iter_mut()
            .find(|seg| seg.section_ndxs.contains(&target_sec_shndx))
            .ok_or_else(|| {
                let err = format!("ELF file error: couldn't find loaded segment that contained section for Rela section {:?}!", sec.get_name(elf_file));
                error!("{}", err);
                err
            })?;
        
        let mut target_segment_dependencies: Vec<StrongDependency> = Vec::new();
        let target_segment_start_addr = target_segment.bounds.start;
        let target_segment_slice: &mut [u8] = target_segment.mp.as_slice_mut(
            0,
            target_segment.bounds.end.value() - target_segment.bounds.start.value(),
        )?;

        // iterate through each relocation entry in the relocation array for the target_sec
        for rela_entry in rela_array {
            use xmas_elf::symbol_table::{Type, Entry};
            let source_sec_entry = &symtab[rela_entry.get_symbol_table_index() as usize];

            // Ignore relocations that refer/point to irrelevant things: sections, files, notypes, or nothing.
            match source_sec_entry.get_type() {
                Err(_) | Ok(Type::NoType) | Ok(Type::Section) | Ok(Type::File) => continue,
                _ => { } // keep going to process the relocation
            }
            if verbose_log {
                trace!("      Rela64 entry has offset: {:#X}, addend: {:#X}, symtab_index: {}, type: {:#X}", 
                    rela_entry.get_offset(), rela_entry.get_addend(), rela_entry.get_symbol_table_index(), rela_entry.get_type());
            }

            let source_sec_shndx = source_sec_entry.shndx() as usize; 
            let source_sec_name = match source_sec_entry.get_name(elf_file) {
                Ok(name) => name,
                _ => continue,
            };

            if verbose_log { 
                let source_sec_header_name = source_sec_entry.get_section_header(elf_file, rela_entry.get_symbol_table_index() as usize)
                    .and_then(|s| s.get_name(elf_file));
                trace!("             --> Points to relevant section [{}]: {:?}", source_sec_shndx, source_sec_header_name);
                trace!("                 Entry name {} {:?} vis {:?} bind {:?} type {:?} shndx {} value {} size {}", 
                    source_sec_entry.name(), source_sec_entry.get_name(elf_file), 
                    source_sec_entry.get_other(), source_sec_entry.get_binding(), source_sec_entry.get_type(), 
                    source_sec_entry.shndx(), source_sec_entry.value(), source_sec_entry.size());
            }

            let demangled = demangle(source_sec_name).to_string();

            // If the source section exists in this namespace already, rewrite the relocation entry to point to the existing section instead.
            if let Some(existing_source_sec) = namespace.get_symbol_or_load(&demangled, None, mmi, verbose_log).upgrade() {
                let mut relocation_entry = RelocationEntry::from_elf_relocation(rela_entry);
                let original_relocation_offset = relocation_entry.offset;
                
                // Here, in an executable ELF file, the relocation entry's "offset" represents an absolute virtual address
                // rather than an offset from the beginning of the section/segment (I think).
                // Therefore, we need to adjust that value before we invoke `write_relocation()`, 
                // which expects a regular `offset` + an offset into the target segment's mapped pages. 
                let relocation_offset_as_vaddr = VirtualAddress::new(relocation_entry.offset).ok_or_else(|| 
                    format!("relocation_entry.offset {:#X} was not a valid virtual address", relocation_entry.offset)
                )?;
                let offset_into_target_segment = relocation_offset_as_vaddr.value() - target_segment_start_addr.value();
                // Now that we have incorporated the relocation_entry's actual offset into the target_segment offset,
                // we set it to zero for the duration of this call. 
                // TODO: this is hacky as hell, we should just create a new `write_relocation()` function instead.
                relocation_entry.offset = 0;

                if verbose_log { 
                    debug!("                 Performing relocation target {:#X} + {:#X} <-- source {}", 
                        target_segment_start_addr, offset_into_target_segment, existing_source_sec.name
                    );
                }
                write_relocation(
                    relocation_entry,
                    target_segment_slice,
                    offset_into_target_segment,
                    existing_source_sec.virt_addr,
                    verbose_log
                )?;
                relocation_entry.offset = original_relocation_offset;
    
                // Here, we typically tell the existing_source_sec that the target_segment is dependent upon it.
                // However, the `WeakDependent` entry type only accepts a weak section reference at the moment,
                // and we don't have that -- we only have a target segment. 
                // TODO: if/when we wish to track weak dependencies from a section to a target segment, we should add it below.
                //
                // let weak_dep = WeakDependent {
                //     section: Arc::downgrade(&target_sec),
                //     relocation: relocation_entry,
                // };
                // existing_source_sec.inner.write().sections_dependent_on_me.push(weak_dep);
                
                // tell the target_sec that it has a strong dependency on the existing_source_sec
                let strong_dep = StrongDependency {
                    section: Arc::clone(&existing_source_sec),
                    relocation: relocation_entry,
                };
                target_segment_dependencies.push(strong_dep);          
            } else {
                trace!("Skipping relocation that points to non-Theseus section: {:?}", demangled);
            }
        }

        // debug!("Target segment dependencies: {:#X?}", target_segment_dependencies);
        target_segment.sections_i_depend_on.append(&mut target_segment_dependencies);
    }

    Ok(())
}

/// Converts the given ELF program flags into `PteFlags`.
fn convert_to_pte_flags(prog_flags: xmas_elf::program::Flags) -> PteFlags {
    PteFlags::new()
        .valid(prog_flags.is_read())
        .writable(prog_flags.is_read())
        .executable(prog_flags.is_execute())
}

fn print_usage(opts: Options) {
    println!("{}", opts.usage(USAGE));
}

const USAGE: &str = "Usage: loadc [ARGS] PATH
Loads C language ELF executables on Theseus.";
