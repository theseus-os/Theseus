#![no_std]

extern crate alloc;

use alloc::{string::String, vec::Vec};
use app_io::println;
use dreadnought::{block_on, FutureExt};

pub fn main(_: Vec<String>) -> isize {
    block_on(async {
        println!("Hello, asynchronous world!");
    });

    block_on(async {
        let result = dreadnought::select_biased! {
            result = dreadnought::future::pending() => result,
            result = foo().fuse() => result,
        };
        assert_eq!(result, 42);
    });

    0
}

async fn foo() -> u8 {
    println!("called foo");
    42
}
