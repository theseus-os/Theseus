//! A Rust version of the shbench heap microbenchmark
//! 
//! The original version can be found on MicroQuill's website
//! http://www.microquill.com/smartheap/shbench/bench.zip

use alloc::{
    vec::Vec,
    string::String,
    alloc::{GlobalAlloc, Layout},
};
use core::sync::atomic::{AtomicUsize, Ordering};
use core::ptr;
use hpet::get_hpet;
use libtest::hpet_2_ns;
use heap::ALLOCATOR;
use crate::NTHREADS;

pub(crate) static NITERATIONS: AtomicUsize = AtomicUsize::new(1000);
pub(crate) static MAX_BLOCK_SIZE: AtomicUsize = AtomicUsize::new(MAX_REGULAR);
pub(crate) static MIN_BLOCK_SIZE: AtomicUsize = AtomicUsize::new(MIN_REGULAR);
/// The default smallest size of object to allocate
const MIN_REGULAR: usize = 1;
/// The default maximum size of object to allocate
const MAX_REGULAR: usize = 1000;
/// The minimum size allocated when the large allocations option is chosen
pub const MIN_LARGE: usize = 8192;
/// The maximum size allocated when the large allocations option is chosen
pub const MAX_LARGE: usize = 16384;


pub fn do_shbench() -> Result<(), &'static str> {

    let nthreads = NTHREADS.load(Ordering::SeqCst);
    let mut threads = Vec::with_capacity(nthreads);
    let niterations = NITERATIONS.load(Ordering::SeqCst);
    let hpet = get_hpet(); 
    println!("Running shbench for {} threads, {} total iterations, {} iterations per thread, {} max block size, {} min block size ...", 
        nthreads, niterations, niterations/nthreads, MAX_BLOCK_SIZE.load(Ordering::SeqCst), MIN_BLOCK_SIZE.load(Ordering::SeqCst));

    let start = hpet.as_ref().ok_or("couldn't get HPET timer")?.get_counter();

    for _ in 0..nthreads {
        threads.push(spawn::new_task_builder(worker, ()).name(String::from("worker thread")).spawn()?);
    }  

    for i in 0..nthreads {
        threads[i].join()?;
        threads[i].take_exit_value();
    }

    let end = hpet.as_ref().ok_or("couldn't get HPET timer")?.get_counter();
    println!("shbench took {} ns", hpet_2_ns(end - start));

    Ok(())
}


fn worker(_:()) {
    let nthreads = NTHREADS.load(Ordering::SeqCst);
    let niterations = NITERATIONS.load(Ordering::SeqCst) / nthreads;
    // the total number of allocations that will be stored at one time
    let alloc_count = niterations;
    let mut allocations = Vec::with_capacity(alloc_count);
    let mut layouts = Vec::with_capacity(alloc_count);
    let min_block_size = MIN_BLOCK_SIZE.load(Ordering::SeqCst);
    let max_block_size = MAX_BLOCK_SIZE.load(Ordering::SeqCst);

    // starting index of the buffer
    let mut mp = 0;
    // max index of the buffer
    let mut mpe = alloc_count;
    // starting index of the portion of allocations that will not be freed in an iteration
    let mut save_start = 0;
    // ending index of the portion of allocations that will not be freed in an iteration
    let mut save_end = 0;

    // initialize the vectors so we can treat them like arrays 
    for _ in 0..alloc_count {
        allocations.push(ptr::null_mut());
        layouts.push(Layout::new::<u8>());
    }

    for _ in 0..niterations {
        let mut size_base = min_block_size;
        while size_base < max_block_size {
            let mut size = size_base;
            while size > 0 {
                let mut iterations = 1;

                // smaller sizes will be allocated a larger amount
                if size < 10000 { iterations = 10; }
                if size < 1000 { iterations *= 5; }
                if size < 100 {iterations *= 5; }

                for _ in 0..iterations {
                    let layout = Layout::from_size_align(size, 2).unwrap();
                    let ptr = unsafe{ ALLOCATOR.alloc(layout) };

                    if ptr.is_null() {
                        error!("Out of Heap Memory");
                        return;
                    }

                    allocations[mp] = ptr;
                    layouts[mp] = layout;
                    mp += 1;

                    // start storing new allocations after the region of pointers that have been saved
                    if mp == save_start {
                        mp = save_end;
                    }

                    // reached the end of the buffer, so now free all allocations except a portion marked by 
                    // save_start and save_end
                    if mp >= mpe {
                        mp = 0;
                        save_start = save_end;
                        if save_start >= mpe {
                            save_start = mp;
                        }
                        save_end = save_start + (alloc_count/5);
                        if save_end > mpe {
                            save_end = mpe;
                        }
                        // free the top part of the buffer, the oldest allocations first
                        while mp < save_start {
                            unsafe { ALLOCATOR.dealloc(allocations[mp], layouts[mp]); }
                            mp += 1;
                        }
                        mp = mpe;
                        // free the end of the buffer, the newest allocations first
                        while mp > save_end {
                            mp -= 1;
                            unsafe { ALLOCATOR.dealloc(allocations[mp], layouts[mp]); }
                        }
                        mp = 0;
                    }
                }
                size /= 2;
            }
            size_base = size_base * 3 / 2 + 1
        }
    }

    //free residual allocations
    mpe = mp;
    mp = 0;

    while mp < mpe {
        unsafe{ ALLOCATOR.dealloc(allocations[mp], layouts[mp]); }
    }        
}



