#![no_std]

#[macro_use] extern crate vga_buffer; // for println_unsafe!
// extern crate nano_core;


pub fn test_lib_func(arg: u8) -> u8 { 
	println_unsafe!("yo in pub func!");
	arg * 10
}