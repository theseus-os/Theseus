//! An application that loads C language executables and libraries atop Theseus.
//!
//! This will be integrated into the Theseus kernel in the future, 
//! likely as a separate crate that 

#![no_std]
#![feature(slice_fill)]

#[macro_use] extern crate alloc;
#[macro_use] extern crate log;
#[macro_use] extern crate terminal_print;
extern crate getopts;
extern crate fs_node;
extern crate path;
extern crate memory;
extern crate mod_mgmt;
extern crate task;
extern crate xmas_elf;
extern crate libc; // for basic C types/typedefs used in libc

use core::{
    cmp::{min, max},
    ops::{AddAssign, SubAssign, Range},
};
use alloc::{collections::BTreeSet, string::String, sync::Arc, vec::Vec};
use getopts::{Matches, Options};
use memory::{Page, MappedPages, VirtualAddress, EntryFlags};
use mod_mgmt::{StrongDependency, find_symbol_table};
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
        Ok(_) => 0,
        Err(e) => {
            println!("Error:\n{}", e);
            -1
        }    
    }
}


fn rmain(matches: Matches) -> Result<(), String> {
    let curr_wd = Arc::clone(
        &task::get_my_current_task().unwrap()
            .lock().env
            .lock().working_dir
    );

    // temporarily dumping page allocator state
    memory::dump_frame_allocator_state();

    let path = matches.free.get(0).ok_or_else(|| format!("Missing path to ELF executable"))?;
    let path = Path::new(path.clone());
    let file_ref = path.get_file(&curr_wd)
        .ok_or_else(|| format!("Failed to access file at {:?}", path))?;
    let file = file_ref.lock();

    // Parse the file as an ELF executable
    let file_mp = file.as_mapping().map_err(|e| String::from(e))?;
    let byte_slice: &[u8] = file_mp.as_slice(0, file.size())?;
    let (mut segments, entry_point, elf_file) = parse_and_load_elf_executable(byte_slice)?;
    debug!("Parsed ELF executable, moving on to overwriting relocations.");
    
    // Now, overwrite (recalculate) the relocations that refer to symbols that already exist in Theseus,
    // most important of which are static data sections, 
    // as it is logically incorrect to have duplicates of data that are supposed to be global system-wide singletons.
    // We should throw a warning here if there are no relocations in the file, as it was probably built/linked with the wrong arguments.
    overwrite_relocations(&mut segments, &elf_file, true)?;

    // Remap each segment's mapped pages using the correct flags; they were previously mapped as always writable.
    {
        let mmi = &task::get_my_current_task().unwrap().lock().mmi;
        let page_table = &mut mmi.lock().page_table;
        for segment in segments.iter_mut() {
            if segment.mp.flags() != segment.flags {
                segment.mp.remap(page_table, segment.flags)?;
            }
        }
    }

    let exec = LoadedExecutable { segments, entry_point }; // must persist through the entire executable's runtime.
    
    debug!("Jumping to entry point {:#X}", entry_point);

    let dummy_args = ["hello", "world"];
    let dummy_env = ["USER=root", "PWD=/"];

    // TODO: FIXME: use `MappedPages::as_func()` instead of `transmute()`.
    let start_fn: StartFunction = unsafe { core::mem::transmute(entry_point.value()) };
    let _retval = start_fn(&dummy_args, &dummy_env);

    debug!("C _start entry point returned value {}({:#X})", _retval, _retval);

    Ok(())
}

/// Corresponds to C function:  `int foo()`
use libc::c_int;
type StartFunction = fn(args: &[&str], env: &[&str]) -> c_int;

struct LoadedExecutable {
    segments: Vec<MappedSegment>,
    entry_point: VirtualAddress,
}


#[derive(Debug)]
pub struct MappedSegment {
    /// The memory region allocated to hold this program segment.
    mp: MappedPages,
    /// The specific range of virtual addresses occupied by this 
    /// (may be a subset)
    bounds: Range<VirtualAddress>,
    /// The proper flags for this segment specified by the ELF file.
    flags: EntryFlags,
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
/// ## Important note about memory mappings
/// This function will allocate new memory regions to store each program segment
/// and copy each segment's data into them.
/// When this function returns, those segments will be mapped as writable in order to allow them 
/// to be modified as needed.
/// Before running this executable, each segment's `MappedPages` should be remapped
/// to the proper `flags` specified in its `MappedSegment.flags` field. 
///
/// ## Return
/// Returns a tuple of:
/// 1. A list of program segments mapped into memory. 
/// 2. The virtual address of the executable's entry point, e.g., the `_start` function.
///    This is the function that we should call to start running the executable.
/// 3. A reference to the parsed `ElfFile`, whose lifetime is tied to the given `file_contents` parameter.
fn parse_and_load_elf_executable<'f>(
    file_contents: &'f [u8],
) -> Result<(Vec<MappedSegment>, VirtualAddress, ElfFile<'f>), String> {
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
        VirtualAddress::new(start_vaddr).map_err(String::from)?,
        total_size_in_bytes
    ).map_err(|_| format!("Failed to allocate {} bytes at {}", total_size_in_bytes, start_vaddr))?;
    let vaddr_adjustment = Offset::new(all_pages.start_address().value(), start_vaddr); 

    // Iterate through each segment again and map them into pages we just allocated above,
    // copying their segment data to the proper location.
    for (segment_ndx, prog_hdr) in elf_file.program_iter().enumerate() {
        debug!("\nLooking at {}", prog_hdr);
        if prog_hdr.get_type() != Ok(xmas_elf::program::Type::Load) {
            warn!("Skipping non-LOAD segment {:?}", prog_hdr);
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
            warn!("Skipping zero-sized LOAD segment {:?}", prog_hdr);
            continue; 
        }

        let mut start_vaddr = VirtualAddress::new(prog_hdr.virtual_addr() as usize)
            .map_err(|_e| {
                error!("Program header virtual address was invalid: {:?}", prog_hdr);
                "Program header had an invalid virtual address"
            })?;
        Offset::adjust_assign(&mut start_vaddr, vaddr_adjustment);
        let end_page = Page::containing_address(start_vaddr + (memory_size_in_bytes - 1));

        debug!("Splitting {:?} after end page {:?}", all_pages, end_page);

        let (this_ap, remaining_pages) = all_pages.split(end_page + 1).map_err(|_ap|
            format!("Failed to split allocated pages {:?} at page {:#X}", _ap, start_vaddr)
        )?;
        all_pages = remaining_pages;
        debug!("Successfully split pages into {:?} and {:?}", this_ap, all_pages);
        debug!("Adjusted segment vaddr: {:#X}, size: {:#X}, {:?}", start_vaddr, memory_size_in_bytes, this_ap.start_address());

        let initial_flags = EntryFlags::from_elf_program_flags(prog_hdr.flags());
        let mmi = task::get_my_current_task().unwrap().lock().mmi.clone();
        // Must initially map the memory as writable so we can copy the segment data to it later. 
        let mut mp = mmi.lock().page_table
            .map_allocated_pages(this_ap, initial_flags | EntryFlags::WRITABLE)
            .map_err(String::from)?;

        // Copy data from this section into the correct offset into our newly-mapped pages
        let offset_into_mp = mp.offset_of_address(start_vaddr).ok_or_else(|| 
            format!("BUG: destination address {:#X} wasn't within segment's {:?}", start_vaddr, mp)
        )?;
        match prog_hdr.get_data(&elf_file).map_err(String::from)? {
            SegmentData::Undefined(segment_data) => {
                debug!("Segment had undefined data of {} ({:#X}) bytes, file size {} ({:#X})",
                    segment_data.len(), segment_data.len(), file_size_in_bytes, file_size_in_bytes);
                let dest_slice: &mut [u8] = mp.as_slice_mut(offset_into_mp, memory_size_in_bytes).map_err(String::from)?;
                dest_slice[..file_size_in_bytes].copy_from_slice(&segment_data[..file_size_in_bytes]);
                if memory_size_in_bytes > file_size_in_bytes {
                    debug!("    Zero-filling extra bytes for segment from range [{}:{}).", file_size_in_bytes, dest_slice.len());
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

        debug!("Segment {} contains sections: {:?}", segment_ndx, section_ndxs);

        mapped_segments.push(MappedSegment {
            mp,
            bounds: segment_bounds,
            flags: initial_flags,
            section_ndxs,
            sections_i_depend_on: Vec::new(),
        });
    }

    let entry_point = elf_file.header.pt2.entry_point() as usize;
    let mut entry_point_vaddr = VirtualAddress::new(entry_point)
        .map_err(|_e| format!("ELF entry point was invalid virtual address: {:#X}", entry_point))?;
    Offset::adjust_assign(&mut entry_point_vaddr, vaddr_adjustment);
    debug!("ELF had entry point {:#X}, adjusted to {:#X}", entry_point, entry_point_vaddr);

    Ok((mapped_segments, entry_point_vaddr, elf_file))
}



/// This function uses the relocation sections in the given `ElfFile` to 
/// rewrite relocations that depend on source sections already existing and currently loaded in Theseus. 
///
/// This is necessary to ensure that the newly-loaded ELF executable depends on and references 
/// the real singleton instances of each data sections (aka `OBJECT`s in ELF terminology) 
/// rather than using the duplicate instance of those data sections in the executable itself. 
fn overwrite_relocations(segments: &mut Vec<MappedSegment>, elf_file: &ElfFile, verbose_log: bool) -> Result<(), String> {
    let symtab = find_symbol_table(&elf_file)?;

    // Fix up the sections that were just loaded, using proper relocation info.
    // Iterate over every non-zero relocation section in the file
    for sec in elf_file.section_iter().filter(|sec| sec.get_type() == Ok(ShType::Rela) && sec.size() != 0) {
        use xmas_elf::sections::SectionData::Rela64;
        if verbose_log { 
            trace!("Found Rela section name: {:?}, type: {:?}, target_sec_index: {:?}", 
            sec.get_name(&elf_file), sec.get_type(), sec.info()); 
        }

        let rela_sec_name = sec.get_name(&elf_file).unwrap();
        // Skip debug special sections for now, those can be processed later. 
        if rela_sec_name.starts_with(".rela.debug")  { 
            continue;
        }
        // Skip .eh_frame relocations, since they are all local to the .text section
        // and cannot depend on external symbols directly
        if rela_sec_name == ".rela.eh_frame"  { 
            continue;
        }

        let rela_array = match sec.get_data(&elf_file) {
            Ok(Rela64(rela_arr)) => rela_arr,
            _ => {
                error!("Found Rela section that wasn't able to be parsed as Rela64: {:?}", sec);
                return Err(format!("Found Rela section that wasn't able to be parsed as Rela64"));
            } 
        };

        // The target section is where we write the relocation data to.
        // The source section is where we get the data from. 
        // There is one target section per rela section (`rela_array`), and one source section per rela_entry in this rela section.
        // The "info" field in the Rela section specifies which section is the target of the relocation.
            
        // Get the target section (that we already loaded) for this rela_array Rela section.
        let target_sec_shndx = sec.info() as usize;
        let target_segment = segments.iter_mut()
            .find(|seg| seg.section_ndxs.contains(&target_sec_shndx))
            .ok_or_else(|| {
                let err = format!("ELF file error: couldn't find loaded segment that contained section for Rela section {:?}!", sec.get_name(&elf_file));
                error!("{}", err);
                err
            })?;
        
        debug!("In {:?}, target sec shndx {} led to relevant segment at {:#X}, which contains sections {:?}", rela_sec_name, target_sec_shndx, target_segment.bounds.start, target_segment.section_ndxs);

        let mut target_segment_dependencies: Vec<StrongDependency> = Vec::new();

        // iterate through each relocation entry in the relocation array for the target_sec
        for rela_entry in rela_array {
            use xmas_elf::symbol_table::{Type, Entry};
            let source_sec_entry = &symtab[rela_entry.get_symbol_table_index() as usize];
            // Currently we only rewrite relocations that refer/point to symbols with an OBJECT type (static data sections).
            if source_sec_entry.get_type() != Ok(Type::Object) {
                continue; 
            }
            if verbose_log {
                trace!("      Object-type Rela64 entry has offset: {:#X}, addend: {:#X}, symtab_index: {}, type: {:#X}", 
                    rela_entry.get_offset(), rela_entry.get_addend(), rela_entry.get_symbol_table_index(), rela_entry.get_type());
            }

            let source_sec_shndx = source_sec_entry.shndx() as usize; 
            if verbose_log { 
                let source_sec_header_name = source_sec_entry.get_section_header(&elf_file, rela_entry.get_symbol_table_index() as usize)
                    .and_then(|s| s.get_name(&elf_file));
                trace!("             --> Points to relevant section [{}]: {:?}", source_sec_shndx, source_sec_header_name);
                trace!("                 Entry name {} {:?} vis {:?} bind {:?} type {:?} shndx {} value {} size {}", 
                    source_sec_entry.name(), source_sec_entry.get_name(&elf_file), 
                    source_sec_entry.get_other(), source_sec_entry.get_binding(), source_sec_entry.get_type(), 
                    source_sec_entry.shndx(), source_sec_entry.value(), source_sec_entry.size());
            }

            /* 
            let mut source_and_target_in_same_crate = false;

            // We first try to get the source section from loaded_sections, which works if the section is in the crate currently being loaded.
            let source_sec = match new_crate.sections.get(&source_sec_shndx) {
                Some(ss) => {
                    source_and_target_in_same_crate = true;
                    Ok(ss.clone())
                }

                // If we couldn't get the section based on its shndx, it means that the source section wasn't in the crate currently being loaded.
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
                        let demangled = demangle(source_sec_name).to_string();

                        // search for the symbol's demangled name in the kernel's symbol map
                        self.get_symbol_or_load(&demangled, temp_backup_namespace, kernel_mmi_ref, verbose_log)
                            .upgrade()
                            .ok_or("Couldn't get symbol for foreign relocation entry, nor load its containing crate")
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
            write_relocation(
                relocation_entry,
                &mut target_segment.mp,
                target_sec.mapped_pages_offset,
                source_sec.start_address(),
                verbose_log
            )?;

            if !source_and_target_in_same_crate {
                // tell the source_sec that the target_sec is dependent upon it
                let weak_dep = WeakDependent {
                    section: Arc::downgrade(&target_sec),
                    relocation: relocation_entry,
                };
                source_sec.inner.write().sections_dependent_on_me.push(weak_dep);
                
                // tell the target_sec that it has a strong dependency on the source_sec
                let strong_dep = StrongDependency {
                    section: Arc::clone(&source_sec),
                    relocation: relocation_entry,
                };
                target_segment_dependencies.push(strong_dep);          
            }

            */
        }

        target_segment.sections_i_depend_on.append(&mut target_segment_dependencies);
    }

    Err(format!("unfinished"))
}


fn print_usage(opts: Options) {
    println!("{}", opts.usage(USAGE));
}


const USAGE: &'static str = "Usage: cload [ARGS] PATH
Loads C language ELF executables or libraries on Theseus.";
