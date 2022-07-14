#![no_std]
// #![feature(plugin)]
// #![plugin(application_main_fn)]

extern crate alloc;

use chrono::naive::NaiveDateTime;
use alloc::{string::String, vec::Vec};
use terminal_print::println;

pub fn main(_args: Vec<String>) {
    let now = time::now::<time::Realtime>();
    let now = NaiveDateTime::from_timestamp(now.as_secs() as i64, now.subsec_nanos());
    println!("{}", now);
}
