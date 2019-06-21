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


use alloc::string::String;
use alloc::vec::Vec;



cfg_if! {
if #[cfg(mapper_spillful)] {


use core::ops::DerefMut;
use getopts::{Matches, Options};
use hpet::get_hpet;
use kernel_config::memory::PAGE_SIZE;


use memory::{FRAME_ALLOCATOR, FrameAllocator, VirtualAddress, Mapper, MappedPages, Page, EntryFlags, mapped_pages_unmap};
use memory::mapper_spillful::MapperSpillful;


enum MapperType<'m> {
    Normal(&'m mut Mapper),
    Spillful(&'m mut MapperSpillful),
}


fn create_mappings(
    mut mapper: MapperType, 
    start_vaddr: VirtualAddress, 
    size_in_pages: usize, 
    num_mappings: usize
) -> Result<Option<Vec<MappedPages>>, &'static str> {

    let mut mapped_pages: Vec<MappedPages> = match mapper {
        MapperType::Normal(_)   => Vec::with_capacity(num_mappings),
        MapperType::Spillful(_) => Vec::new(),
    };
    let size_in_bytes = size_in_pages * PAGE_SIZE;


    let (tsc_ticks, hpet_ticks) = {
        let mut frame_allocator_ref = FRAME_ALLOCATOR.try().ok_or("Couldn't get FRAME_ALLOCATOR")?.lock();
        let frame_allocator = frame_allocator_ref.deref_mut();

        let start_hpet = get_hpet().as_ref().ok_or("couldn't get HPET timer")?.get_counter();
        let start_tsc = tsc::tsc_ticks();

        for i in 0..num_mappings {
            let vaddr = start_vaddr + i*size_in_bytes;
            match mapper {
                MapperType::Normal(ref mut mapper) => {
                    let mp = mapper.map_pages(
                        PageRange::from_virt_addr(vaddr, size_in_bytes),
                        EntryFlags::WRITABLE | EntryFlags::PRESENT,
                        frame_allocator
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

        let end_tsc = tsc::tsc_ticks();
        let end_hpet = get_hpet().as_ref().ok_or("couldn't get HPET timer")?.get_counter();

        (end_tsc.into() - start_tsc.into(), end_hpet - start_hpet)
    };
    

    println!("Created {} {}-byte mappings ({}) in {} TSC ticks, {} HPET ticks.", 
        num_mappings, 
        size_in_bytes, 
        match mapper { 
            MapperType::Normal(_)   =>  "NORMAL",
            MapperType::Spillful(_) =>  "SPILLFUL",
        },
        tsc_ticks,
        hpet_ticks,
    );

  
    match mapper {
        MapperType::Normal(_)   => Ok(Some(mapped_pages)),
        MapperType::Spillful(_) => Ok(None),
    }

}


fn remap_normal(
    mapper_normal: &mut Mapper, 
    mapped_pages: &mut Vec<MappedPages>,
) -> Result<(), &'static str> {

    let num_mappings = mapped_pages.len();

    let (tsc_ticks, hpet_ticks) = {
        let start_hpet = get_hpet().as_ref().ok_or("couldn't get HPET timer")?.get_counter();
        let start_tsc = tsc::tsc_ticks();

        for mut mp in mapped_pages.iter_mut() {
            let _res = mp.remap(mapper_normal, EntryFlags::PRESENT)?;
        }

        let end_tsc = tsc::tsc_ticks();
        let end_hpet = get_hpet().as_ref().ok_or("couldn't get HPET timer")?.get_counter();
        (end_tsc.into() - start_tsc.into(), end_hpet - start_hpet)
    };
    

    println!("Remapped {} mappings ({}) in {} TSC ticks, {} HPET ticks.", 
        num_mappings, 
        "NORMAL",
        tsc_ticks,
        hpet_ticks
    );

    Ok(())
}



fn remap_spillful(
    mapper_spillful: &mut MapperSpillful, 
    start_vaddr: VirtualAddress, 
    size_in_pages: usize, 
    num_mappings: usize
) -> Result<(), &'static str> {

    let size_in_bytes = size_in_pages * PAGE_SIZE;

    let (tsc_ticks, hpet_ticks) = {
        let start_hpet = get_hpet().as_ref().ok_or("couldn't get HPET timer")?.get_counter();
        let start_tsc = tsc::tsc_ticks();

        for i in 0..num_mappings {
            let vaddr = start_vaddr + i*size_in_pages*PAGE_SIZE;            
            let _res = mapper_spillful.remap(vaddr, EntryFlags::PRESENT)?;
        }

        let end_tsc = tsc::tsc_ticks();
        let end_hpet = get_hpet().as_ref().ok_or("couldn't get HPET timer")?.get_counter();
        (end_tsc.into() - start_tsc.into(), end_hpet - start_hpet)
    };
    

    println!("Remapped {} {}-byte mappings ({}) in {} TSC ticks, {} HPET ticks.", 
        num_mappings, 
        size_in_bytes, 
        "SPILLFUL",
        tsc_ticks,
        hpet_ticks
    );

    Ok(())
}


fn unmap_normal(mapper_normal: &mut Mapper, mut mapped_pages: Vec<MappedPages>) -> Result<(), &'static str> {
    let mut frame_allocator_ref = FRAME_ALLOCATOR.try().ok_or("Couldn't get FRAME_ALLOCATOR")?.lock();
    let frame_allocator = frame_allocator_ref.deref_mut();
    let num_mappings = mapped_pages.len();

    let (tsc_ticks, hpet_ticks) = {
        let start_hpet = get_hpet().as_ref().ok_or("couldn't get HPET timer")?.get_counter();
        let start_tsc = tsc::tsc_ticks();

        for mp in &mut mapped_pages {
            mapped_pages_unmap(mp, mapper_normal, frame_allocator)?;
        }

        let end_tsc = tsc::tsc_ticks();
        let end_hpet = get_hpet().as_ref().ok_or("couldn't get HPET timer")?.get_counter();
        (end_tsc.into() - start_tsc.into(), end_hpet - start_hpet)
    };

    // To avoid measuring the (irrelevant) overhead of vector allocation/deallocation, we manually unmapped the MappedPages above. 
    // Thus, here we "forget" each MappedPages from the vector to ensure that their Drop handlers aren't called.
    while let Some(mp) = mapped_pages.pop() {
        core::mem::forget(mp); 
    }

    println!("Unmapped {} mappings ({}) in {} TSC ticks, {} HPET ticks.", 
        num_mappings, 
        "NORMAL",
        tsc_ticks,
        hpet_ticks
    );

    Ok(())
}


fn unmap_spillful(
    mapper_spillful: &mut MapperSpillful, 
    start_vaddr: VirtualAddress, 
    size_in_pages: usize, 
    num_mappings: usize
) -> Result<(), &'static str> {

    let mut frame_allocator_ref = FRAME_ALLOCATOR.try().ok_or("Couldn't get FRAME_ALLOCATOR")?.lock();
    let frame_allocator = frame_allocator_ref.deref_mut();

    let size_in_bytes = size_in_pages * PAGE_SIZE;

    let (tsc_ticks, hpet_ticks) = {
        let start_hpet = get_hpet().as_ref().ok_or("couldn't get HPET timer")?.get_counter();
        let start_tsc = tsc::tsc_ticks();

        for i in 0..num_mappings {
            let vaddr = start_vaddr + i*size_in_pages*PAGE_SIZE;            
            let _res = mapper_spillful.unmap(vaddr, frame_allocator)?;
        }

        let end_tsc = tsc::tsc_ticks();
        let end_hpet = get_hpet().as_ref().ok_or("couldn't get HPET timer")?.get_counter();
        (end_tsc.into() - start_tsc.into(), end_hpet - start_hpet)
    };
    

    println!("Unmapped {} {}-byte mappings ({}) in {} TSC ticks, {} HPET ticks.", 
        num_mappings, 
        size_in_bytes, 
        "SPILLFUL",
        tsc_ticks,
        hpet_ticks
    );

    Ok(())
}


#[no_mangle]
pub fn main(args: Vec<String>) -> isize {

    let mut opts = Options::new();
    opts.optflag("h", "help", "print this help menu");
    opts.optflag("p", "spillful", "run the state spillful memory mapping evaluation");
    opts.optopt("n", "", "create 'N' mappings ", "NUM");
    opts.optopt("s", "--size", "specify the size (in bytes) for each mapping", "SIZE");

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

    let result = rmain(&matches, &opts);
    match result {
        Ok(_) => { 0 }
        Err(e) => {
            println!("Memory mapping evaluation failed: {}.", e);
            -1
        }
    }
}


pub fn rmain(matches: &Matches, opts: &Options) -> Result<(), &'static str> {

    let mut mapper_normal   = Mapper::from_current();
    let mut mapper_spillful = MapperSpillful::new();

    let start_vaddr = 0xFFFF_FA00_0000_0000; // the start of the 500th P4 (PML4) entry

    let num_mappings = matches.opt_str("n")
        .and_then(|i| i.parse::<usize>().ok())
        .unwrap_or(100);

    let size_in_pages = matches.opt_str("s")
        .and_then(|i| i.parse::<usize>().ok())
        .unwrap_or(2);

    let use_spillful = if matches.opt_present("p") {
        true
    } else {
        false
    };


    {
        // (1) create mappings
        let mut result = create_mappings(
            if use_spillful {
                MapperType::Spillful(&mut mapper_spillful)
            } else {
                MapperType::Normal(&mut mapper_normal)
            },
            start_vaddr, 
            size_in_pages, 
            num_mappings
        )?;

        // (2) perform remappings
        match result {
            Some(ref mut mapped_pages) => remap_normal(&mut mapper_normal, mapped_pages)?,
            _                          => remap_spillful(&mut mapper_spillful, start_vaddr, size_in_pages, num_mappings)?,
        };
            
        // (3) perform unmappings
        match result {
            Some(mapped_pages) => unmap_normal(&mut mapper_normal, mapped_pages)?,
            _                  => unmap_spillful(&mut mapper_spillful, start_vaddr, size_in_pages, num_mappings)?,
        };
    }

    let hpet_period = get_hpet().as_ref().ok_or("couldn't get HPET timer")?.counter_period_femtoseconds();
    println!("TSC period: {:?}, HPET Period: {} femtoseconds)", tsc::get_tsc_frequency(), hpet_period);

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
    
#[no_mangle]
pub fn main(_args: Vec<String>) -> isize {
    println!("Error: Theseus was not compiled with the 'mapper_spillful' config enabled, \
              which is required to run this benchmark application.");
    return -1;
}

}
}
