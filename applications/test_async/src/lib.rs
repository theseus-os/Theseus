#![no_std]

extern crate alloc;

use alloc::{string::String, vec::Vec};
use app_io::println;
use dreadnought::{block_on, FutureExt, time, select_biased};

pub fn main(_: Vec<String>) -> isize {
    block_on(async {
        println!("Hello, asynchronous world!");
    });

    block_on(async {
        let result = select_biased! {
            result = foo().fuse() => result,
            result = bar().fuse() => result,
        };
        assert_eq!(result, 1);
    });

    0
}

async fn foo() -> u8 {
    println!("called foo");
    time::sleep(10000).await;
    0
}

async fn bar() -> u8 {
    println!("called bar");
    time::sleep(5000).await;
    1
}
