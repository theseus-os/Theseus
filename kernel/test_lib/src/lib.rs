#![no_std]
// #![feature(lang_items)]

// #[macro_use] extern crate vga_buffer; // for println_unsafe!
// #[macro_use] extern crate log;

// extern crate kernel_config;

// pub static HELLO_STRING: &'static str = "hello";

pub static mut INT_VALUE: usize = 98765;
pub static mut OTHER_VAL: u32 = 123;

pub const HELLO_STRING: &'static str = "hello";

// #[inline(never)]
// pub fn test_generic<T>(arg: T) -> T {
// 	arg
// }


//#[no_mangle]
//#[inline(never)]
//pub fn unmangled_fn(arg: u8) -> u8 {
//	123
//}


#[inline(never)]
pub fn test_lib_public(arg: u8) -> (u8, &'static str, u64) {// , bool) { 
	// warn!("yo in pub func!");
	// test_lib_private(arg)
	// arg * 10
	let test_u8: u8 = arg;
	let test_str: &str = HELLO_STRING;
	let test_u64: u64 = 5u64;
	unsafe { 
		(OTHER_VAL as u8, test_str, INT_VALUE as u64) // , kernel_config::memory::KERNEL_TEXT_START) //, kernel_config::memory::address_is_page_aligned(0x1000))
	}
}

#[inline(never)]
fn test_lib_private(arg: u8) -> u8 {
	arg * 10
}


pub struct DeezNuts {
	item1: u32,
	item2: u64,
}
impl DeezNuts {
	#[inline(never)]
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
