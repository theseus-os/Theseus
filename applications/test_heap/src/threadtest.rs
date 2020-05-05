//! A Rust version of the threadtest heap microbenchmark
//! 
//! The original version was presented in the Hoard paper
//! https://github.com/emeryberger/Hoard/tree/master/benchmarks/threadtest

use alloc::{
    vec::Vec,
    string::String,
    alloc::{GlobalAlloc, Layout}
};
use core::sync::atomic::{Ordering, AtomicUsize};
use core::ptr;
use hpet::get_hpet;
use libtest::hpet_2_ns;
use crate::NTHREADS;
use heap::ALLOCATOR;


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
            let ptr = unsafe{ ALLOCATOR.alloc(layout) };
            allocations[i] = ptr;
        }
        for i in 0..(nobjects/nthreads) {
            unsafe{ ALLOCATOR.dealloc(allocations[i], layout); }
        }
    }
}



