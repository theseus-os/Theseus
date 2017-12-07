#![no_std]

// #[macro_use] extern crate vga_buffer; // for println_unsafe!
// #[macro_use] extern crate log;

pub fn test_lib_func(arg: u8) -> u8 { 
	// warn!("yo in pub func!");
	arg * 10
}