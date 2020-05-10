//! A Rust version of the threadtest heap microbenchmark
//! 
//! The original version was presented in the Hoard paper
//! https://github.com/emeryberger/Hoard/tree/master/benchmarks/threadtest

use alloc::{
    vec::Vec,
    string::String,
    alloc::Layout
};
#[cfg(not(direct_access_to_multiple_heaps))]
use alloc::alloc::GlobalAlloc;
use core::sync::atomic::{Ordering, AtomicUsize};
use core::ptr;
use hpet::get_hpet;
use libtest::{hpet_2_us, calculate_stats, hpet_timing_overhead};
use crate::{NTHREADS, ALLOCATOR, TRIES};
#[cfg(direct_access_to_multiple_heaps)]
use crate::overhead_of_accessing_multiple_heaps;


pub(crate) static NITERATIONS: AtomicUsize = AtomicUsize::new(1000);
/// Sum total of objects to be allocated by all threads
pub(crate) static NOBJECTS: AtomicUsize = AtomicUsize::new(100_000);
/// Size of the objects we're allocating in bytes
pub(crate) static OBJSIZE: AtomicUsize = AtomicUsize::new(REGULAR_SIZE);
/// The default size of objects to allocate
const REGULAR_SIZE: usize = 8;
/// The size allocated when the large allocations option is chosen
pub const LARGE_SIZE: usize = 8192;

pub fn do_threadtest() -> Result<(), &'static str> {

    let nthreads = NTHREADS.load(Ordering::SeqCst);
    let mut tries = Vec::with_capacity(TRIES as usize);

    let hpet_overhead = hpet_timing_overhead()?;
    let hpet_ref = get_hpet(); 
    let hpet = hpet_ref.as_ref().ok_or("couldn't get HPET timer")?;

    println!("Running threadtest for {} threads, {} iterations, {} total objects allocated every iteration by all threads, {} obj size ...", 
        nthreads, NITERATIONS.load(Ordering::SeqCst), NOBJECTS.load(Ordering::SeqCst), OBJSIZE.load(Ordering::SeqCst));

    #[cfg(direct_access_to_multiple_heaps)]
    {
        let overhead = overhead_of_accessing_multiple_heaps()?;
        println!("Overhead of accessing multiple heaps is: {} ticks, {} ns", overhead, hpet_2_us(overhead));
    }

    for try in 0..TRIES {
        let mut threads = Vec::with_capacity(nthreads);

        let start = hpet.get_counter();

        for _ in 0..nthreads {
            threads.push(spawn::new_task_builder(worker, ()).name(String::from("worker thread")).spawn()?);
        }  

        for i in 0..nthreads {
            threads[i].join()?;
        }

        let end = hpet.get_counter() - hpet_overhead;

        // Don't want this to be part of the timing measurement
        for thread in threads {
            thread.take_exit_value();
        }

        let diff = hpet_2_us(end - start);
        println!("[{}] threadtest time: {} us", try, diff);
        tries.push(diff);
    }

    println!("threadtest stats (us)");
    println!("{:?}", calculate_stats(&tries));

    Ok(())
}


fn worker(_:()) {
    #[cfg(not(direct_access_to_multiple_heaps))]
    let allocator = &ALLOCATOR;

    // In the case of directly accessing the multiple heaps, we do have to access them through the Once wrapper
    // at the beginning, but the time it takes to do this once at the beginning of thread is
    // insignificant compared to the number of iterations we run. It also printed above.
    #[cfg(direct_access_to_multiple_heaps)]
    let allocator = match ALLOCATOR.try() {
        Some(allocator) => allocator,
        None => {
            error!("Multiple heaps not initialized!");
            return;
        }
    };

    let niterations = NITERATIONS.load(Ordering::SeqCst);
    let nobjects = NOBJECTS.load(Ordering::SeqCst);
    let nthreads = NTHREADS.load(Ordering::SeqCst);
    let obj_size = OBJSIZE.load(Ordering::SeqCst);

    let mut allocations = Vec::with_capacity(nobjects/nthreads);
    // initialize the vector so we do not measure the time of `push` and `pop`
    for _ in 0..(nobjects / nthreads) {
        allocations.push(ptr::null_mut());
    }
    let layout = Layout::from_size_align(obj_size, 8).unwrap();

    for _ in 0..niterations {
        for i in 0..(nobjects/nthreads) {
            let ptr = unsafe{ allocator.alloc(layout) };
            allocations[i] = ptr;
        }
        for i in 0..(nobjects/nthreads) {
            unsafe{ allocator.dealloc(allocations[i], layout); }
        }
    }
}



