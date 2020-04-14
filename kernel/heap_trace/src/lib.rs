
#![no_std]

#[macro_use] extern crate log;
extern crate hpet;
extern crate pmu_x86;
extern crate x86_64;
extern crate apic;

use hpet::get_hpet;
use pmu_x86::{Counter, EventType, FIXED_FUNC_2_RDPMC};
use x86_64::instructions::rdpmc;
use core::sync::atomic::{AtomicUsize, Ordering};

pub static mut START: bool = false;
pub const ARRAY_SIZE: usize = 30_000 * NSTEPS;
pub static mut HEAP_TRACE:[u64; ARRAY_SIZE] = [0; ARRAY_SIZE]; 
pub static mut ID: usize = 0;
// pub static mut STEP: usize = 0;
pub const NSTEPS: usize = 7;
pub static STEPS_TAKEN: AtomicUsize = AtomicUsize::new(0);


// #[inline(always)]
pub fn take_step() {
    unsafe {
    if START {
        HEAP_TRACE[ID] = rdpmc(FIXED_FUNC_2_RDPMC);
        // trace!("heap trace: {}", HEAP_TRACE[ID]);
        ID += 1;
        // STEP = step;
        STEPS_TAKEN.fetch_add(1, Ordering::SeqCst);
        }
    }
}

pub fn start_heap_trace() {
    // let _ = pmu_x86::init();
    
    unsafe{ 
        ID = 0;
        START = true; 
        STEPS_TAKEN.store(0, Ordering::SeqCst); 
    }
}

pub fn stop_heap_trace() {
    unsafe{
        START = false; 
    }
}


pub fn print_heap_trace_from_index(start_index: usize, end_index: usize) {
    if end_index >= ARRAY_SIZE {
        error!("Index is too large");
        return;
    }

    unsafe {
    for i in start_index..end_index {
        error!("{}", HEAP_TRACE[i]);
    }
    }
}

pub fn calclulate_time_per_step() -> [u64; NSTEPS] {
    let mut avg_times: [u64; NSTEPS] = [0; NSTEPS];

    unsafe {

    let steps_taken = STEPS_TAKEN.load(Ordering::SeqCst);

    if steps_taken > ARRAY_SIZE || steps_taken == 0{
        // error!("Increase array size to : {}", steps_taken);
        return avg_times;
    }

    // find time difference between each step
    for i in (1..steps_taken).rev() {
        HEAP_TRACE[i] = HEAP_TRACE[i] - HEAP_TRACE[i-1];
    }

    // find average time for each step
    let start_index = NSTEPS; //skip the first cycle
    let end_index = (steps_taken / NSTEPS) * NSTEPS;

    // error!("start index: {}, end index: {}", start_index, end_index);


    for step in 0..NSTEPS {
        let mut iter = 0;
        for i in ((start_index + step)..end_index).step_by(NSTEPS) {
            avg_times[step] += HEAP_TRACE[i];
            iter += 1;
        }
        avg_times[step] /= iter;
        // error!("step {} occurred {} times", step, iter);
    }

    // error!("Took a total of {} steps", STEPS_TAKEN);
    // error!("{:?}", avg_times);
    }
    avg_times


}


