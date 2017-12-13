#![no_std]
// #![feature(lang_items)]

// #[macro_use] extern crate vga_buffer; // for println_unsafe!
// #[macro_use] extern crate log;

pub fn test_lib_public(arg: u8) -> u8 { 
	// warn!("yo in pub func!");
	// test_lib_private(arg)
	arg * 10
}

// #[inline(never)]
// fn test_lib_private(arg: u8) -> u8 {
// 	arg * 10
// }


pub struct DeezNuts {
	item1: u32,
	item2: u64,
}
impl DeezNuts {
	pub fn new(i1: u32, i2: u64) -> DeezNuts {
		DeezNuts {
			item1: i1,
			item2: i2,
		}
	}
}


// #[cfg(not(test))]
// #[lang = "eh_personality"]
// extern "C" fn eh_personality() {}


// #[cfg(not(test))]
// #[lang = "panic_fmt"]
// #[no_mangle]
// pub extern "C" fn panic_fmt(_fmt: core::fmt::Arguments, _file: &'static str, _line: u32) -> ! {
//     // println_unsafe!("\n\nPANIC in {} at line {}:", file, line);
//     // println_unsafe!("    {}", fmt);

//     // TODO: check out Redox's unwind implementation: https://github.com/redox-os/kernel/blob/b364d052f20f1aa8bf4c756a0a1ea9caa6a8f381/src/arch/x86_64/interrupt/trace.rs#L9

//     loop {}
// }
