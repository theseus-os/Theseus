#![no_std]

extern crate alloc;

use alloc::{string::String, vec::Vec};
use app_io::println;
use dreadnought::{
    block_on, select_biased,
    task::spawn_async,
    time::{sleep, Duration},
    FutureExt,
};

pub fn main(_: Vec<String>) -> isize {
    block_on(async {
        println!("Hello, asynchronous world!");

        let result = select_biased! {
            result = foo().fuse() => result,
            result = bar().fuse() => result,
        };
        assert_eq!(result, 1);

        let handle_1 = spawn_async(async { 1855 }).unwrap();
        // TODO: Fix task abortion. Aborting the spawned task won't properly clean it up
        // and so test_async won't be dropped.
        // let handle_2 = spawn(async { loop {} }).unwrap();

        assert_eq!(handle_1.await.unwrap(), 1855);
        // handle_2.abort();
        // assert!(matches!(handle_2.await, Err(Error::Cancelled)));

        0
    })
}

async fn foo() -> u8 {
    println!("called foo");
    sleep(Duration::from_secs(2)).await;
    println!("foo sleep done");
    0
}

async fn bar() -> u8 {
    println!("called bar");
    sleep(Duration::from_secs(1)).await;
    println!("bar sleep done");
    1
}
