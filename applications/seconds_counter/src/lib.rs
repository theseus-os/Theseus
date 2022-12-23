#![no_std]

extern crate alloc;

use alloc::{string::String, vec::Vec};

pub fn main(_: Vec<String>) {
    let mut counter = 0;
    let mut previous = time::now::<time::Monotonic>();
    loop {
        let now = time::now::<time::Monotonic>();
        if now.duration_since(previous) >= time::Duration::from_secs(1) {
            counter += 1;
            app_io::println!("{}", counter);
            previous = now;
        }
    }
}
