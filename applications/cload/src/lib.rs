//! An application that loads C language executables and libraries atop Theseus.
//!
//! This will be integrated into the Theseus kernel in the future.

#![no_std]

#[macro_use] extern crate alloc;
#[macro_use] extern crate log;
#[macro_use] extern crate terminal_print;
extern crate getopts;
extern crate fs_node;
extern crate path;
extern crate memory;
extern crate task;
extern crate xmas_elf;
extern crate libc; // for basic C types/typedefs used in libc

use core::{
    cmp::{min, max},
    ops::{AddAssign, SubAssign, Range},
};
use alloc::{
    string::String,
    sync::Arc,
    vec::Vec,
};
use getopts::{Matches, Options};
use memory::{Page, MappedPages, VirtualAddress, EntryFlags};
use path::Path;
use xmas_elf::{
    ElfFile,
    program::SegmentData,
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

    let path = matches.free.get(0).ok_or_else(|| format!("Missing path to ELF executable"))?;
    let path = Path::new(path.clone());
    let file_ref = path.get_file(&curr_wd)
        .ok_or_else(|| format!("Failed to access file at {:?}", path))?;
    let file = file_ref.lock();

    // Parse the file as an ELF executable
    let file_mp = file.as_mapping().map_err(|e| String::from(e))?;
    
    let (segments, entry_point) = parse_elf_executable(file_mp, file.size())?;
    let exec = LoadedExecutable { segments, entry_point };
    
    debug!("Jumping to entry point {:#X}", entry_point);

    let start_fn: StartFunction = unsafe { core::mem::transmute(entry_point.value()) };
    let _retval = start_fn();

    debug!("C _start entry point returned value {}({:#X})", _retval, _retval);

    Ok(())
}

/// Corresponds to C function:  `int foo()`
use libc::c_int;
type StartFunction = extern "C" fn() -> c_int;

struct LoadedExecutable {
    segments: Vec<MappedSegment>,
    entry_point: VirtualAddress,
}


#[derive(Debug)]
pub struct MappedSegment {
    mp: MappedPages,
    bounds: Range<VirtualAddress>,
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


/// Parses an elf executable file as a slice of bytes starting at the given `MappedPages` mapping.
/// Consumes the given `MappedPages`, which automatically unmaps it at the end of this function. 
fn parse_elf_executable(
    mapped_pages: &MappedPages,
    size_in_bytes: usize
) -> Result<(Vec<MappedSegment>, VirtualAddress), String> {
    debug!("Parsing Elf executable: mapped_pages {:?}, size_in_bytes {}", mapped_pages, size_in_bytes);

    let byte_slice: &[u8] = mapped_pages.as_slice(0, size_in_bytes)?;
    let elf_file = ElfFile::new(byte_slice).map_err(String::from)?;

    // check that elf_file is an executable type 
    let typ = elf_file.header.pt2.type_().as_type();
    if typ != xmas_elf::header::Type::Executable {
        error!("parse_elf_executable(): ELF file has wrong type {:?}, must be an Executable Elf File!", typ);
        return Err("not a relocatable elf file".into());
    }

    // The relative position of each segment in memory (with respect to other segments) must be maintained. 
    // Therefore, we iterate over all segments first to find the total range of virtual pages we must allocate. 
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
    let mut all_pages = memory::allocate_pages_by_bytes(total_size_in_bytes)
        .ok_or_else(|| format!("Failed to allocate {} bytes", total_size_in_bytes))?;
    let vaddr_adjustment = Offset::new(all_pages.start_address().value(), start_vaddr); 

    // Iterate through each segment again and map them into pages we just allocated above,
    // copying their segment data to the proper location.
    for prog_hdr in elf_file.program_iter() {
        debug!("   prog_hdr: {}", prog_hdr);
        if prog_hdr.get_type() != Ok(xmas_elf::program::Type::Load) {
            warn!("Skipping non-LOAD segment {:?}", prog_hdr);
            continue;
        }

        let size_in_bytes = prog_hdr.mem_size() as usize;
        if size_in_bytes == 0 {
            warn!("Skipping zero-sized LOAD segment {:?}", prog_hdr);
            continue; 
        }

        let mut start_vaddr = VirtualAddress::new(prog_hdr.virtual_addr() as usize)
            .map_err(|_e| {
                error!("Program header virtual address was invalid: {:?}", prog_hdr);
                "Program header had an invalid virtual address"
            })?;
        Offset::adjust_assign(&mut start_vaddr, vaddr_adjustment);
        let end_page = Page::containing_address(start_vaddr + (size_in_bytes - 1));

        debug!("Splitting {:?} after end page {:?}", all_pages, end_page);

        let (this_ap, remaining_pages) = all_pages.split(end_page + 1).map_err(|_ap|
            format!("Failed to split allocated pages {:?} at page {:#X}", _ap, start_vaddr)
        )?;
        all_pages = remaining_pages;
        debug!("Successfully split pages into {:?} and {:?}", this_ap, all_pages);
        debug!("Adjusted segment vaddr: {:#X}, size: {:#X}, {:?}", start_vaddr, size_in_bytes, this_ap.start_address());

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
                debug!("Segment had undefined data of {} bytes: {:?}", segment_data.len(), segment_data);
                let dest_slice: &mut [u8] = mp.as_slice_mut(offset_into_mp, size_in_bytes).map_err(String::from)?;
                dest_slice.copy_from_slice(segment_data);
            }
            other => {
                warn!("Segment had data of unhandled type: {:?}", other);
            }
        };

        // If needed, remap the mapped pages (which now hold the segment data) using their correct flags.
        if !initial_flags.is_writable() {
            mp.remap(&mut mmi.lock().page_table, initial_flags)?;
        }

        mapped_segments.push(MappedSegment {
            bounds: start_vaddr .. (start_vaddr + size_in_bytes),
            mp,
        });
    }

    let entry_point = elf_file.header.pt2.entry_point() as usize;
    let mut entry_point_vaddr = VirtualAddress::new(entry_point)
        .map_err(|_e| format!("ELF entry point was invalid virtual address: {:#X}", entry_point))?;
    Offset::adjust_assign(&mut entry_point_vaddr, vaddr_adjustment);
    debug!("ELF had entry point {:#X}, adjusted to {:#X}", entry_point, entry_point_vaddr);

    Ok((mapped_segments, entry_point_vaddr))
}


fn print_usage(opts: Options) {
    println!("{}", opts.usage(USAGE));
}


const USAGE: &'static str = "Usage: cload [ARGS] PATH
Loads C language ELF executables or libraries on Theseus.";
