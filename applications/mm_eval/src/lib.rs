//! This application tests the performance of two memory mapping implementations,
//! which is used to compare our spill-free `MappedPages` approach 
//! with a standard memory mapping implementation based on `VirtualMemoryArea`s.

#![no_std]

extern crate alloc;
#[macro_use] extern crate cfg_if;
#[macro_use] extern crate app_io;
extern crate log;
extern crate memory;
extern crate getopts;
extern crate hpet;
extern crate kernel_config;
extern crate tsc;
extern crate memory_structs;
extern crate apic;
extern crate runqueue;

use alloc::string::String;
use alloc::vec::Vec;


cfg_if! {
if #[cfg(mapper_spillful)] {

#[macro_use] extern crate libtest;
extern crate mapper_spillful;

use getopts::{Matches, Options};
use kernel_config::memory::PAGE_SIZE;
use libtest::{hpet_timing_overhead, hpet_2_ns, calculate_stats, check_myrq};
use memory::{allocate_pages, AllocatedPages, VirtualAddress, Mapper, MappedPages, EntryFlags, mapper_from_current, mapped_pages_unmap};
use mapper_spillful::MapperSpillful;
use hpet::get_hpet;

enum MapperType<'m> {
    Normal(&'m mut Mapper),
    Spillful(&'m mut MapperSpillful),
}


fn create_mappings(
    mut mapper: MapperType, 
    allocated_pages: AllocatedPages, 
    size_in_pages: usize, 
    num_mappings: usize,
    hpet_overhead: u64
) -> Result<(Option<Vec<MappedPages>>, Option<Vec<AllocatedPages>>, u64), &'static str> {
    let start_vaddr = allocated_pages.start().start_address();
    // split the allocated pages into `num_mappings` chunks, each chunk being `size_in_pages` pages long.
    let mut allocated_pages_list: Vec<AllocatedPages> = Vec::with_capacity(num_mappings);
    let mut remaining_chunk = allocated_pages;
    for _i in 0..num_mappings {
        let split_at_page = *remaining_chunk.start() + size_in_pages;
        let (small_chunk, big_chunk) = remaining_chunk.split(split_at_page)
            .map_err(|_ap| "failed to split allocated pages")?;
        remaining_chunk = big_chunk;
        allocated_pages_list.push(small_chunk);
    }
    if remaining_chunk.size_in_pages() != 0 {
        return Err("failed to split allocated pages into num_mappings even segments");
    }

    let hpet = get_hpet().ok_or("couldn't get HPET timer")?;

    let mut mapped_pages: Vec<MappedPages> = match mapper {
        MapperType::Normal(_)   => Vec::with_capacity(num_mappings),
        MapperType::Spillful(_) => Vec::new(),
    };
    let size_in_bytes = size_in_pages * PAGE_SIZE;

    let start_time = hpet.get_counter();

    match mapper {
        MapperType::Normal(ref mut mapper) => {
            for allocated_pages in allocated_pages_list {
                let mp = mapper.map_allocated_pages(
                    allocated_pages,
                    EntryFlags::WRITABLE | EntryFlags::PRESENT,
                )?;
                mapped_pages.push(mp);
            }
            let end_time = hpet.get_counter() - hpet_overhead;
            Ok((Some(mapped_pages), None, hpet_2_ns(end_time - start_time)))
        }
        MapperType::Spillful(ref mut mapper) => {
            for i in 0..num_mappings {
                let vaddr = start_vaddr + i*size_in_bytes;
                let _res = mapper.map(vaddr, size_in_bytes,
                    EntryFlags::WRITABLE | EntryFlags::PRESENT,
                )?;
            }
            let end_time = hpet.get_counter() - hpet_overhead;
            Ok((None, Some(allocated_pages_list), hpet_2_ns(end_time - start_time)))
        }
    }
}


fn remap_normal(
    mapper_normal: &mut Mapper, 
    mapped_pages: &mut Vec<MappedPages>,
    hpet_overhead: u64
) -> Result<u64, &'static str> {

    let hpet = get_hpet().ok_or("couldn't get HPET timer")?;
    let start_time = hpet.get_counter();

    for mp in mapped_pages.iter_mut() {
        let _res = mp.remap(mapper_normal, EntryFlags::PRESENT)?;
    }

    let end_time = hpet.get_counter() - hpet_overhead;

    Ok(hpet_2_ns(end_time - start_time))
}


fn remap_spillful(
    mapper_spillful: &mut MapperSpillful, 
    start_vaddr: VirtualAddress, 
    size_in_pages: usize, 
    num_mappings: usize,
    hpet_overhead: u64
) -> Result<u64, &'static str> {

    let hpet = get_hpet().ok_or("couldn't get HPET timer")?;
    let start_time = hpet.get_counter();

    for i in 0..num_mappings {
        let vaddr = start_vaddr + i*size_in_pages*PAGE_SIZE;            
        let _res = mapper_spillful.remap(vaddr, EntryFlags::PRESENT)?;
    }

    let end_time = hpet.get_counter() - hpet_overhead;

    Ok(hpet_2_ns(end_time - start_time))
}


fn unmap_normal(
    mapper_normal: &mut Mapper, 
    mut mapped_pages: Vec<MappedPages>,
    hpet_overhead: u64
) -> Result<u64, &'static str> {

    let hpet = get_hpet().ok_or("couldn't get HPET timer")?;
    let start_time = hpet.get_counter();

    for mp in &mut mapped_pages {
        mapped_pages_unmap(mp, mapper_normal)?;
    }

    let end_time = hpet.get_counter() - hpet_overhead;

    // To avoid measuring the (irrelevant) overhead of vector allocation/deallocation, we manually unmapped the MappedPages above. 
    // Thus, here we "forget" each MappedPages from the vector to ensure that their Drop handlers aren't called.
    while let Some(mp) = mapped_pages.pop() {
        core::mem::forget(mp); 
    }

    Ok(hpet_2_ns(end_time - start_time))
}


fn unmap_spillful(
    mapper_spillful: &mut MapperSpillful, 
    start_vaddr: VirtualAddress, 
    size_in_pages: usize, 
    num_mappings: usize,
    hpet_overhead: u64
) -> Result<u64, &'static str> {

    let hpet = get_hpet().ok_or("couldn't get HPET timer")?;

    let start_time = hpet.get_counter();

    for i in 0..num_mappings {
        let vaddr = start_vaddr + i*size_in_pages*PAGE_SIZE;            
        let _res = mapper_spillful.unmap(vaddr)?;
    }

    let end_time = hpet.get_counter() - hpet_overhead;

    Ok(hpet_2_ns(end_time - start_time))
}


pub fn main(args: Vec<String>) -> isize {

    let mut opts = Options::new();
    opts.optflag("h", "help", "print this help menu");
    opts.optflag("v", "verbose", "enable verbose output");
    opts.optflag("p", "spillful", "run the state spillful memory mapping evaluation");
    opts.optopt("n", "", "create 'N' mappings ", "NUM");
    opts.optopt("s", "--size", "specify the size (in pages) for each mapping", "SIZE");

    let matches = match opts.parse(&args) {
        Ok(m) => m,
        Err(_f) => {
            println!("{}", _f);
            print_usage(&opts);
            return -1; 
        }
    };

    if matches.opt_present("h") {
        print_usage(&opts);
        return 0;
    }

    if !check_myrq() {
		println!("mm_eval cannot run on a busy core (#{}). Pin me on an idle core.", CPU_ID!());
		return 0;
	}

    let result = rmain(&matches, &opts);
    match result {
        Ok(_) => { 0 }
        Err(e) => {
            println!("Memory mapping evaluation failed: {}.", e);
            -1
        }
    }
}


pub fn rmain(matches: &Matches, _opts: &Options) -> Result<(), &'static str> {
    const TRIES: usize = 10;
    let mut mapper_normal   = mapper_from_current();
    let mut mapper_spillful = MapperSpillful::new();

    let num_mappings = matches.opt_str("n")
        .and_then(|i| i.parse::<usize>().ok())
        .unwrap_or(100);

    let size_in_pages = matches.opt_str("s")
        .and_then(|i| i.parse::<usize>().ok())
        .unwrap_or(2);

    let use_spillful = matches.opt_present("p");
    let verbose = matches.opt_present("v");

    if num_mappings < 1 { 
        return Err("invalid number of mappings specified.")
    }

    if size_in_pages < 1 { 
        return Err("invalid size of each mapping specified.")
    }

    println!("Running mm_eval with {} {}-page mappings, evaluated {} times. Spillful? {}.",
        num_mappings, size_in_pages, TRIES, use_spillful
    );

    let mut create_times: Vec<u64> = Vec::with_capacity(TRIES);
    let mut remap_times: Vec<u64> = Vec::with_capacity(TRIES);
    let mut unmap_times: Vec<u64> = Vec::with_capacity(TRIES);

    // calculate overhead of reading hpet counter
    let overhead = hpet_timing_overhead()?;

    for _trial in 0..TRIES {
        let pages = allocate_pages(size_in_pages * num_mappings)
            .ok_or("couldn't allocate sufficient pages")?;
        let start_vaddr = pages.start_address();
        
        // (1) create mappings
        if verbose { println!("*** Iteration {}: creating mappings.", _trial); }         
        let mut result = create_mappings(
            if use_spillful {
                MapperType::Spillful(&mut mapper_spillful)
            } else {
                MapperType::Normal(&mut mapper_normal)
            },
            pages, 
            size_in_pages, 
            num_mappings,
            overhead
        )?;

        // (2) perform remappings
        if verbose { println!("*** Iteration {}: performing remappings.", _trial); }         
        match result {
            (Some(ref mut mapped_pages), None, time) => {
                create_times.push(time);
                let remap = remap_normal(&mut mapper_normal, mapped_pages, overhead)?;
                remap_times.push(remap);
            }
            (None, Some(ref _allocated_pages_list), time) => {
                create_times.push(time);
                let remap = remap_spillful(&mut mapper_spillful, start_vaddr, size_in_pages, num_mappings, overhead)?;
                remap_times.push(remap);
            }
            _ => return Err("BUG: create_mappings returned unexpected result"),
        };
            
        // (3) perform unmappings
        if verbose { println!("*** Iteration {}: performing unmappings.", _trial); }         
        match result {
            (Some(mapped_pages), None, _time) => {
                let unmap = unmap_normal(&mut mapper_normal, mapped_pages, overhead)?;
                unmap_times.push(unmap);
            }
            (None, Some(_allocated_pages_list), _time) => {  
                let unmap = unmap_spillful(&mut mapper_spillful, start_vaddr, size_in_pages, num_mappings, overhead)?;
                unmap_times.push(unmap);
            }
            _ => return Err("BUG: create_mappings returned unexpected result"),
        };
    }

    println!("Create Mappings (ns)");
    let stats_create = calculate_stats(&mut create_times).ok_or("Could not calculate stats for mappings")?;
    println!("{:?}", stats_create);

    println!("Remap Mappings (ns)");
    let stats_remap = calculate_stats(&mut remap_times).ok_or("Could not calculate stats for remappings")?;
    println!("{:?}", stats_remap);
    
    println!("Unmap Mappings (ns)");
    let stats_unmap = calculate_stats(&mut unmap_times).ok_or("Could not calculate stats for unmappings")?;
    println!("{:?}", stats_unmap);

    Ok(())

}


fn print_usage(opts: &Options) {
    println!("{}", opts.usage(USAGE));
}


const USAGE: &'static str = "Usage: mm_eval [ARGS]
Evaluates two different memory mapping implementations.
The normal spill-free MappedPages approach is evaluated by default.";

} // end of cfg_if
else {
    
pub fn main(_args: Vec<String>) -> isize {
    println!("Error: Theseus was not compiled with the 'mapper_spillful' config enabled, \
              which is required to run this benchmark application.");
    return -1;
}

}
}
