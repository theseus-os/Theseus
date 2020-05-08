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
use libtest::hpet_2_ns;
use crate::{NTHREADS, ALLOCATOR};
#[cfg(direct_access_to_multiple_heaps)]
use crate::overhead_of_accessing_multiple_heaps;


pub(crate) static NITERATIONS: AtomicUsize = AtomicUsize::new(50);
/// Sum total of objects to be allocated by all threads
pub(crate) static NOBJECTS: AtomicUsize = AtomicUsize::new(30_000);
/// Size of the objects we're allocating in bytes
pub(crate) static OBJSIZE: AtomicUsize = AtomicUsize::new(REGULAR_SIZE);
/// The default size of objects to allocate
const REGULAR_SIZE: usize = 8;
/// The size allocated when the large allocations option is chosen
pub const LARGE_SIZE: usize = 8192;

pub fn do_threadtest() -> Result<(), &'static str> {

    let nthreads = NTHREADS.load(Ordering::SeqCst);
    let mut threads = Vec::with_capacity(nthreads);
    let hpet = get_hpet(); 
    println!("Running threadtest for {} threads, {} iterations, {} total objects, {} obj size ...", 
        nthreads, NITERATIONS.load(Ordering::SeqCst), NOBJECTS.load(Ordering::SeqCst), OBJSIZE.load(Ordering::SeqCst));

    #[cfg(direct_access_to_multiple_heaps)]
    {
        let overhead = overhead_of_accessing_multiple_heaps()?;
        println!("Overhead of accessing multiple heaps is: {} ticks, {} ns", overhead, hpet_2_ns(overhead));
    }

    let start = hpet.as_ref().ok_or("couldn't get HPET timer")?.get_counter();

    for _ in 0..nthreads {
        threads.push(spawn::new_task_builder(worker, ()).name(String::from("worker thread")).spawn()?);
    }  

    for i in 0..nthreads {
        threads[i].join()?;
        threads[i].take_exit_value();
    }

    let end = hpet.as_ref().ok_or("couldn't get HPET timer")?.get_counter();
    println!("threadtest took {} ns", hpet_2_ns(end - start));

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



