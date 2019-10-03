#![no_std]

#[macro_use] extern crate log;
extern crate libc;
extern crate mod_mgmt;
extern crate memory;
#[macro_use] extern crate alloc;
extern crate spawn;

use mod_mgmt::get_default_namespace;
use alloc::string::String;
use spawn::KernelTaskBuilder;

// #[link(name = "test")]
// extern {
//    // fn register_callback(cb: extern fn(usize) -> *mut u8) -> i32;
//    fn testing_lc(a: i32) -> i32;
// }


pub fn test() -> Result<u64, &'static str>{

	let kernel_mmi_ref = memory::get_kernel_mmi_ref().ok_or_else(|| "couldn't get kernel mmi")?;

   // we're going to manually load the crate and symbol
	let backup_namespace = get_default_namespace().ok_or("default crate namespace wasn't yet initialized")?;
   
   let libc = backup_namespace.get_kernel_file_starting_with("libc-").ok_or("couldn't find libc crate")?;
   backup_namespace.load_kernel_crate(&libc, None, &kernel_mmi_ref, true)?;

   let my_c_program = backup_namespace.get_kernel_file_starting_with("my_c_program").ok_or("couldn't find c program crate")?;
   backup_namespace.load_kernel_crate(&my_c_program, None, &kernel_mmi_ref, true)?;

   type SimdTestFunc = fn(i32)->i32;
	let section_ref1 = backup_namespace.get_symbol_starting_with("testing_lc")
		.upgrade()
		.ok_or("no single symbol matching \"testing_lc\"")?;
	let mut space1 = 0;	
	let (mapped_pages1, mapped_pages_offset1) = { 
		let section = section_ref1.lock();
		(section.mapped_pages.clone(), section.mapped_pages_offset)
	};
   debug!("Mapped pages: {:#X}, Mapped pages offset: {}", mapped_pages1.lock().start_address(), mapped_pages_offset1);
	let func1: &SimdTestFunc = mapped_pages1.lock().as_func(mapped_pages_offset1, &mut space1)?;
   let res = func1(0);
   // TODO: find out why spawning doesn't work!!!
	// let task1 = KernelTaskBuilder::new(func1, 0)
	// 	.spawn()?;
	debug!("finished spawning my_c_program::testing_lc: {}", res);
   
   // unsafe{
   //    // let a = register_callback(libc::rmalloc);
   //    let b = testing_lc(0);

   //    // debug!("Test libc: {}, {}", a, b)
   //    debug!("Test libc: {}", b);

   // }

   Ok(0)

}

// void *mmap(void *addr, size_t length, int prot, int flags, int fd, off_t offset);
// fn mmap(addr: *mut u8, length: size_t, prot: i32, flags: i32, fd: i32, offset: off_t) -> *mut u8 {

// }

// fn munmap
