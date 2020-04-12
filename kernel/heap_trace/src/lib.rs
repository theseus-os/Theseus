
#![no_std]

#[macro_use] extern crate log;

pub static mut START: bool = false;
pub const ARRAY_SIZE: usize = 1_000_000;
pub static mut HEAP_TRACE:[u64; ARRAY_SIZE] = [0; ARRAY_SIZE]; 
pub static mut ID: usize = 0;
pub static mut STEP: usize = 0;


pub fn start_heap_trace() {
    unsafe{ START = true; }
}

pub fn stop_heap_trace() {
    unsafe{ START = false; }
}

pub fn print_heap_trace_from_index(index: usize) {
    if index >= ARRAY_SIZE {
        error!("Index is too large");
        return;
    }

    unsafe {
    for i in index..ARRAY_SIZE {
        error!("{}", HEAP_TRACE[i]);
    }
    }
}



