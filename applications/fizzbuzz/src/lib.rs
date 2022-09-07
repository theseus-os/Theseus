#![no_std]
// #![feature(plugin)]
// #![plugin(application_main_fn)]


extern crate alloc;
// #[macro_use] extern crate log;
#[macro_use] extern crate terminal_print;

use alloc::vec::Vec;
use alloc::string::String;
use alloc::string::ToString;

pub fn main(_args: Vec<String>) -> isize {
    // info!("Hello, world! (from hello application)");
    for x in 1..=100_i64 {
        println!("{}",
            if x%15==0 {"FizzBuzz".to_string()}
            else if x%3==0 {"Fizz".to_string()}
            else if x%5==0 {"Buzz".to_string()}
            else {x.to_string()}
        );
    }

    0
}
