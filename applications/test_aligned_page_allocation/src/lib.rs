//! A set of basic tests for the [`AllocationRequest::AlignedTo`] variant.

#![no_std]

extern crate alloc;

use alloc::{
    vec::Vec,
    string::String,
};
use app_io::println;
use memory::AllocationRequest;

static TEST_SET: [usize; 9] = [1, 2, 4, 8, 27, 48, 256, 512, 1024];

pub fn main(_args: Vec<String>) -> isize {    
    match rmain() {
        Ok(_) => 0,
        Err(e) => {
            println!("Error: {}", e); 
            -1
        }
    }
}

fn rmain() -> Result<(), &'static str> {
    for num_pages in TEST_SET.into_iter() {
        for alignment in TEST_SET.into_iter() {
            println!("Attempting to allocate {num_pages} pages with alignment of {alignment} 4K pages...");
            match memory::allocate_pages_deferred(
                AllocationRequest::AlignedTo { alignment_4k_pages: alignment },
                num_pages,
            ) {
                Ok((ap, _action)) => {
                    assert_eq!(ap.start().number() % alignment, 0);
                    assert_eq!(ap.size_in_pages(), num_pages);
                    println!("    Success: {ap:?}");
                }
                Err(e) => println!("    !! FAILURE: {e:?}"),
            }
        }
    }
    
    Ok(())
}
