#![no_std]
#[macro_use] extern crate terminal_print;

extern crate alloc;
use alloc::vec::Vec;
use alloc::string::String;

pub fn main(args: Vec<String>) -> isize{
    let cow_front = "     _________\n
    < ";

    let cow_back = " >\n
     ---------\n
            \\   ^__^\n
             \\  (oo)\\_______\n
                (__)\\       )\\/\\\n
                    ||----w |\n
                    ||     ||\n";

    let mut cow_string = String::from(cow_front);
    let mut clone_args = args.clone();
    let input = match clone_args.pop() {
        Some(s) => s,
        None => String::from("MoOoOoO")
    };
    cow_string.push_str(&input);
    cow_string.push_str(cow_back);

    println!("{}", cow_string);
    0
}