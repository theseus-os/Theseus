//! This application tests the performance of two memory mapping implementations,
//! which is used to compare our spill-free `MappedPages` approach 
//! with a standard memory mapping implementation based on `VirtualMemoryArea`s.

#![no_std]

extern crate alloc;
#[macro_use] extern crate cfg_if;
#[macro_use] extern crate terminal_print;
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
use memory_structs::PageRange;
use libtest::{hpet_timing_overhead, hpet_2_ns, calculate_stats, check_myrq};
use memory::{get_frame_allocator_ref, VirtualAddress, Mapper, MappedPages, EntryFlags, mapped_pages_unmap};
use mapper_spillful::MapperSpillful;
use hpet::get_hpet;

enum MapperType<'m> {
    Normal(&'m mut Mapper),
    Spillful(&'m mut MapperSpillful),
}


fn create_mappings(
    mut mapper: MapperType, 
    start_vaddr: VirtualAddress, 
    size_in_pages: usize, 
    num_mappings: usize,
    hpet_overhead: u64
) -> Result<(Option<Vec<MappedPages>>, u64), &'static str> {

    let mut mapped_pages: Vec<MappedPages> = match mapper {
        MapperType::Normal(_)   => Vec::with_capacity(num_mappings),
        MapperType::Spillful(_) => Vec::new(),
    };
    let size_in_bytes = size_in_pages * PAGE_SIZE;

    let frame_allocator = get_frame_allocator_ref().ok_or("Couldn't get frame allocator")?;
    let hpet = get_hpet().ok_or("couldn't get HPET timer")?;

    let start_time = hpet.get_counter();

    for i in 0..num_mappings {
        let vaddr = start_vaddr + i*size_in_bytes;
        match mapper {
            MapperType::Normal(ref mut mapper) => {
                let mp = mapper.map_pages(
                    PageRange::from_virt_addr(vaddr, size_in_bytes),
                    EntryFlags::WRITABLE | EntryFlags::PRESENT,
                    &mut *frame_allocator.lock(),
                )?;
                mapped_pages.push(mp);
            }
            MapperType::Spillful(ref mut mapper) => {
                let _res = mapper.map(vaddr, size_in_bytes,
                    EntryFlags::WRITABLE | EntryFlags::PRESENT,
                    frame_allocator
                )?;
            }
        }
    }

    let end_time = hpet.get_counter() - hpet_overhead;
  
    match mapper {
        MapperType::Normal(_)   => Ok((Some(mapped_pages), hpet_2_ns(end_time - start_time))),
        MapperType::Spillful(_) => Ok((None, hpet_2_ns(end_time - start_time))),
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

    let frame_allocator = get_frame_allocator_ref().ok_or("Couldn't get frame allocator")?;
    let hpet = get_hpet().ok_or("couldn't get HPET timer")?;
    let start_time = hpet.get_counter();

    for mp in &mut mapped_pages {
        mapped_pages_unmap(mp, mapper_normal, frame_allocator)?;
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

    let frame_allocator = get_frame_allocator_ref().ok_or("Couldn't get frame allocator")?;
    let hpet = get_hpet().ok_or("couldn't get HPET timer")?;

    let start_time = hpet.get_counter();

    for i in 0..num_mappings {
        let vaddr = start_vaddr + i*size_in_pages*PAGE_SIZE;            
        let _res = mapper_spillful.unmap(vaddr, frame_allocator)?;
    }

    let end_time = hpet.get_counter() - hpet_overhead;

    Ok(hpet_2_ns(end_time - start_time))
}


pub fn main(args: Vec<String>) -> isize {

    let mut opts = Options::new();
    opts.optflag("h", "help", "print this help menu");
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
    let mut mapper_normal   = Mapper::from_current();
    let mut mapper_spillful = MapperSpillful::new();

    let start_vaddr = 0xFFFF_FA00_0000_0000; // the start of the 500th P4 (PML4) entry

    let mut num_mappings = matches.opt_str("n")
        .and_then(|i| i.parse::<usize>().ok())
        .unwrap_or(100);

    let size_in_pages = matches.opt_str("s")
        .and_then(|i| i.parse::<usize>().ok())
        .unwrap_or(1);

    let mut use_spillful = if matches.opt_present("p") {
        true
    } else {
        false
    };

    let mut create_times: Vec<u64> = Vec::with_capacity(TRIES);
    let mut remap_times: Vec<u64> = Vec::with_capacity(TRIES);
    let mut unmap_times: Vec<u64> = Vec::with_capacity(TRIES);

    // calculate overhead of reading hpet counter
    let overhead = hpet_timing_overhead()?;

    let num_tests = 8;
    let test_sizes = [100,100,1000,1000,10000,10000,100000,100000];
    let spillful_test = [false,true,false,true,false,true,false,true];
    let mut map_mean = Vec::with_capacity(num_tests);
    let mut map_std_dev = Vec::with_capacity(num_tests);
    let mut remap_mean = Vec::with_capacity(num_tests);
    let mut remap_std_dev = Vec::with_capacity(num_tests);
    let mut unmap_mean = Vec::with_capacity(num_tests);
    let mut unmap_std_dev = Vec::with_capacity(num_tests);


    for i in 0..num_tests {
        let num_mappings = test_sizes[i];
        let use_spillful = spillful_test[i];

        for _ in 0..TRIES 
        {
            // (1) create mappings
            let mut result = create_mappings(
                if use_spillful {
                    MapperType::Spillful(&mut mapper_spillful)
                } else {
                    MapperType::Normal(&mut mapper_normal)
                },
                VirtualAddress::new(start_vaddr)?, 
                size_in_pages, 
                num_mappings,
                overhead
            )?;

            // (2) perform remappings
            match result {
                (Some(ref mut mapped_pages), time) => {
                    create_times.push(time);
                    let remap = remap_normal(&mut mapper_normal, mapped_pages, overhead)?;
                    remap_times.push(remap);
                }
                (None, time) => {
                    create_times.push(time);
                    let remap = remap_spillful(&mut mapper_spillful, VirtualAddress::new(start_vaddr)?, size_in_pages, num_mappings, overhead)?;
                    remap_times.push(remap);
                }
            };
                
            // (3) perform unmappings
            match result {
                (Some(mapped_pages), _time) => {
                    let unmap = unmap_normal(&mut mapper_normal, mapped_pages, overhead)?;
                    unmap_times.push(unmap);
                }
                (None, _time) => {  
                    let unmap = unmap_spillful(&mut mapper_spillful, VirtualAddress::new(start_vaddr)?, size_in_pages, num_mappings, overhead)?;
                    unmap_times.push(unmap);
                }
            };
        }

        // println!("Create Mappings (ns)");
        let stats_create = calculate_stats(&mut create_times).ok_or("Could not calculate stats for mappings")?;
        // println!("{:?}", stats_create);

        // println!("Remap Mappings (ns)");
        let stats_remap = calculate_stats(&mut remap_times).ok_or("Could not calculate stats for remappings")?;
        // println!("{:?}", stats_remap);
        
        // println!("Unmap Mappings (ns)");
        let stats_unmap = calculate_stats(&mut unmap_times).ok_or("Could not calculate stats for unmappings")?;
        // println!("{:?}", stats_unmap);

        map_mean.push(stats_create.mean);
        map_std_dev.push(stats_create.std_dev);
        remap_mean.push(stats_remap.mean);
        remap_std_dev.push(stats_remap.std_dev);
        unmap_mean.push(stats_unmap.mean);
        unmap_std_dev.push(stats_unmap.std_dev);

        create_times.clear();
        remap_times.clear();
        unmap_times.clear();
    }

    /// Print Out Table
    println!("Memory Mapping Benchmark Results (from Fig 3)");
    println!("");
    println!("");

    println!("Mapping Type   Total Mappings   Map Mean (ns)   Map Std Dev (ns)");
    println!("__________________________________________________________________");
    println!("MappedPages     {:.3}              {:.3}            {:.3}", test_sizes[0], map_mean[0] / test_sizes[0] as f64, map_std_dev[0] / test_sizes[0] as f64);
    println!("VMAs            {:.3}              {:.3}            {:.3}", test_sizes[1], map_mean[1] / test_sizes[1] as f64, map_std_dev[1] / test_sizes[1] as f64);
    println!("__________________________________________________________________");
    println!("MappedPages     {:.3}             {:.3}            {:.3}", test_sizes[2], map_mean[2] / test_sizes[2] as f64, map_std_dev[2] / test_sizes[2] as f64);
    println!("VMAs            {:.3}             {:.3}            {:.3}", test_sizes[3], map_mean[3] / test_sizes[3] as f64, map_std_dev[3] / test_sizes[3] as f64);
    println!("__________________________________________________________________");
    println!("MappedPages     {:.3}            {:.3}            {:.3}", test_sizes[4], map_mean[4] / test_sizes[4] as f64, map_std_dev[4] / test_sizes[4] as f64);
    println!("VMAs            {:.3}            {:.3}            {:.3}", test_sizes[5], map_mean[5] / test_sizes[5] as f64, map_std_dev[5] / test_sizes[5] as f64);
    println!("_________________________________________________________________");
    println!("MappedPages     {:.3}           {:.3}            {:.3}", test_sizes[6], map_mean[6] / test_sizes[6] as f64, map_std_dev[6] / test_sizes[6] as f64);
    println!("VMAs            {:.3}           {:.3}            {:.3}", test_sizes[7], map_mean[7] / test_sizes[7] as f64, map_std_dev[7] / test_sizes[7] as f64);


    println!("");
    println!("");


    println!("Mapping Type   Total Mappings   Remap Mean (ns)   Remap Std Dev (ns)");
    println!("__________________________________________________________________");
    println!("MappedPages     {:.3}                {:.3}           {:.3}", test_sizes[0], remap_mean[0] / test_sizes[0] as f64, remap_std_dev[0] / test_sizes[0] as f64);
    println!("VMAs            {:.3}                {:.3}           {:.3}", test_sizes[1], remap_mean[1] / test_sizes[1] as f64, remap_std_dev[1] / test_sizes[1] as f64);
    println!("__________________________________________________________________");
    println!("MappedPages     {:.3}               {:.3}           {:.3}", test_sizes[2], remap_mean[2] / test_sizes[2] as f64, remap_std_dev[2] / test_sizes[2] as f64);
    println!("VMAs            {:.3}               {:.3}           {:.3}", test_sizes[3], remap_mean[3] / test_sizes[3] as f64, remap_std_dev[3] / test_sizes[3] as f64);
    println!("__________________________________________________________________");
    println!("MappedPages     {:.3}              {:.3}           {:.3}", test_sizes[4], remap_mean[4] / test_sizes[4] as f64, remap_std_dev[4] / test_sizes[4] as f64);
    println!("VMAs            {:.3}              {:.3}           {:.3}", test_sizes[5], remap_mean[5] / test_sizes[5] as f64, remap_std_dev[5] / test_sizes[5] as f64);
    println!("_________________________________________________________________");
    println!("MappedPages     {:.3}             {:.3}           {:.3}", test_sizes[6], remap_mean[6] / test_sizes[6] as f64, remap_std_dev[6] / test_sizes[6] as f64);
    println!("VMAs            {:.3}             {:.3}           {:.3}", test_sizes[7], remap_mean[7] / test_sizes[7] as f64, remap_std_dev[7] / test_sizes[7] as f64);

    println!("");
    println!("");

    println!("Mapping Type   Total Mappings   Unmap Mean (ns)   Unmap Std Dev (ns)");
    println!("__________________________________________________________________");
    println!("MappedPages     {:.3}                {:.3}           {:.3}", test_sizes[0], unmap_mean[0] / test_sizes[0] as f64, unmap_std_dev[0] / test_sizes[0] as f64);
    println!("VMAs            {:.3}                {:.3}           {:.3}", test_sizes[1], unmap_mean[1] / test_sizes[1] as f64, unmap_std_dev[1] / test_sizes[1] as f64);
    println!("__________________________________________________________________");
    println!("MappedPages     {:.3}               {:.3}           {:.3}", test_sizes[2], unmap_mean[2] / test_sizes[2] as f64, unmap_std_dev[2] / test_sizes[2] as f64);
    println!("VMAs            {:.3}               {:.3}           {:.3}", test_sizes[3], unmap_mean[3] / test_sizes[3] as f64, unmap_std_dev[3] / test_sizes[3] as f64);
    println!("__________________________________________________________________");
    println!("MappedPages     {:.3}              {:.3}           {:.3}", test_sizes[4], unmap_mean[4] / test_sizes[4] as f64, unmap_std_dev[4] / test_sizes[4] as f64);
    println!("VMAs            {:.3}              {:.3}           {:.3}", test_sizes[5], unmap_mean[5] / test_sizes[5] as f64, unmap_std_dev[5] / test_sizes[5] as f64);
    println!("_________________________________________________________________");
    println!("MappedPages     {:.3}             {:.3}           {:.3}", test_sizes[6], unmap_mean[6] / test_sizes[6] as f64, unmap_std_dev[6] / test_sizes[6] as f64);
    println!("VMAs            {:.3}             {:.3}           {:.3}", test_sizes[7], unmap_mean[7] / test_sizes[7] as f64, unmap_std_dev[7] / test_sizes[7] as f64);

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
