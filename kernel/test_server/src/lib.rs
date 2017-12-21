#![no_std]


pub mod test;

#[inline(never)]
pub fn generic_fn<T: Clone>(arg: T) -> T {
	arg.clone()
}


//#[no_mangle]
//#[inline(never)]
//pub fn unmangled_fn(arg: u8) -> u8 {
//	123
//}


// pub fn server_func<'a>(arg1: u8, arg2: &'a str) -> (u8, &'a str) {
#[inline(never)]
pub fn server_func(arg1: u8, arg2: u64) -> (u8, u64) {
	(test::another_file_fn(arg1), arg2 * 2)
}

pub struct MyStruct {
	item1: u32,
	item2: u64,
}
impl MyStruct {
	#[inline(never)]
	pub fn new(i1: u32, i2: u64) -> MyStruct {
		MyStruct {
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
