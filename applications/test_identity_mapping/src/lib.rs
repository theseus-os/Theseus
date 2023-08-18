//! A set of basic tests for [`memory::create_identity_mapping()`].

#![no_std]

extern crate alloc;

use alloc::{
    vec::Vec,
    string::String,
};
use app_io::println;

static TEST_SET: [usize; 8] = [1, 2, 4, 8, 25, 48, 256, 1024];

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
    let flags = memory::PteFlags::new().valid(true);
    for num_pages in TEST_SET.into_iter() {
        println!("Attempting to create identity mapping of {num_pages} pages...");
        match memory::create_identity_mapping(num_pages, flags) {
            Ok(mp) => {
                assert_eq!(mp.size_in_pages(), num_pages);
                println!("    Success: {mp:?}");
            }
            Err(e) => println!("    !! FAILURE: {e:?}"),
        }
    }
    
    Ok(())
}
