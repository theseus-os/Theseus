//! Management of two kernel personalities, one for SIMD-enabled code, and one for regular code. 
//! 
//! This crate is responsible for creating and managing multiple `CrateNamespace`s so that Theseus
//! can run two instances of code side-by-side, like library OS-style personalities. 
//! One personality is the regular code 
//! 
//! The policy for determining this is determined per-core, as follows:     
//! * If there is one or zero SIMD-enabled `Task`s running on a given core, 
//!   then all `Task`s running on that core can use the standard, 
//!   non-SIMD-enabled context switching routine. 
//!   This is because there is no need to save and restore SIMD registers (e.g., xmm),
//!   as only a single `Task` will ever use them. 
//! * If there are multiple SIMD-enabled `Task`s running on a given core,
//!   then all `Task`s on that core must use the SIMD-enabled context switching routine,
//!   which is found in the `context_switch_sse` crate. 
//! 
//! This also needs to extend to interrupt handlers, which will need to exist in two fashions: 
//! * An interrupt handler that saves/restores both regular and SIMD registers,
//! * An interrupt handler that saves/restores only regular registers and NOT any SIMD registers. 
//! 
//! 
//! # Considerations
//! The ideal, most-efficient setup would be to save SIMD registers ONLY when switching *away* 
//! from a SIMD `Task`, and to restore SIMD registers when switching *to* a SIMD `Task`. 
//! Then, regular (non-SIMD) `Task`s would only need to save/restore regular registers, 
//! since they cannot modify those SIMD registers. 
//! 
// //! However, this is practically impossible, because the context switch routine 
// //! needs to push the same number of registers onto the stack as it will later pop off of the stack. 
// //! So, when switching from a regular `Task` to a SIMD `Task`, the regular `Task` would 
// //! if some tasks need to push 7 registers onto the stack, while others needs to op
//!
//! 
//! # CORRECTION! 
//! Actually, it is possible to do this. The only thing we'd have to switch using personalities 
//! is the interrupt handlers.
//! Also, we'd still need separate personalities to allow SIMD code to run side-by-side with non-SIMD code. 
//! 
//! Each Task can have its own Context and context_switch routine, based on whether or not it uses SIMD instructions. 
//! We don't really need personalities for that, since the `context_switch` routines are self-contained. 
//! 
//! However, we still have the issue of interrupt handlers needing to change, using this policy:     
//! * zero SIMD Tasks on a core: use regular interrupt handlers. 
//! * one or more SIMD Tasks on a core, use SIMD interrupt handlers.      
//! Although a SIMD-enabled interrupt handler would be very slow, I think that it would actually work and be correct. 
//! 
//! Another thing that we could do is just prohibit all interrupt handlers from using SIMD code, 
//! but I currently don't have a way to check this. 
//! For now, I guess we could just force interrupt handlers to be compiled without SIMD support, 
//! and then only use those handlers for both personalities. 
//! 
//! So we don't actually need per-core IDTs. We just need 

#![no_std]
#![feature(compiler_builtins_lib)]
#![feature(alloc)]

// this is needed for symbols like memcpy, which aren't in the core lib
// extern crate rlibc; 
#[cfg(target_feature = "sse2")]
extern crate compiler_builtins as unused_compiler_builtins;


#[macro_use] extern crate alloc;
#[macro_use] extern crate log;
extern crate memory;
extern crate mod_mgmt;
extern crate spawn;


use core::ops::{Deref, DerefMut};
use memory::{get_kernel_mmi_ref, get_module, MemoryManagementInfo};
use mod_mgmt::CrateNamespace;
use alloc::String;


const SSE_KERNEL_PREFIX: &'static str = "k_sse#";


pub fn init_simd_personality(_: ()) -> Result<(), &'static str> {
	let kernel_mmi_ref = get_kernel_mmi_ref().ok_or_else(|| "couldn't get kernel mmi")?;

	let backup_namespace = mod_mgmt::get_default_namespace();
	let simd_namespace = CrateNamespace::with_name("simd");

	// // load the simd_core, which is like the nano_core, in that it bootstraps the new SIMD personality
	// let simd_core = get_module("k_sse#simd_core").ok_or_else(|| "couldn't get k_sse#simd_core module")?;
	// let _num_syms = simd_namespace.load_kernel_crate(simd_core, None, kernel_mmi_ref.lock().deref_mut(), true)?;
	// debug!("init_simd_personality(): loaded simd_core, {} new symbols.", _num_syms);
	// debug!("After loading simd_core, here are the symbols: {:?}", simd_namespace.dump_symbol_map());


	// Because the panic_unwind and core library object files have a circular dependency,
	// (as do the other libraries below), we need to load them all at the same time.
	let compiler_builtins_simd = get_module("k_sse#compiler_builtins").ok_or_else(|| "couldn't get k_sse#compiler_builtins module")?;
	let rlibc_simd = get_module("k_sse#rlibc").ok_or_else(|| "couldn't get k_sse#rlibc module")?;
	let core_lib_simd = get_module("k_sse#core").ok_or_else(|| "couldn't get k_sse#core module")?;
	let panic_unwind_simd = get_module("k_sse#panic_unwind_simple").ok_or_else(|| "couldn't get k_sse#panic_unwind_simple module")?;

	// // but we first need to load the allocator symbols
	// let rust_alloc   = backup_namespace.get_symbol("__rust_alloc")  .upgrade().ok_or("couldn't get __rust_alloc symbol")?;
	// let rust_dealloc = backup_namespace.get_symbol("__rust_dealloc").upgrade().ok_or("couldn't get __rust_dealloc symbol")?;
	// let rust_oom     = backup_namespace.get_symbol("__rust_oom")    .upgrade().ok_or("couldn't get __rust_oom symbol")?;
	// let rust_realloc = backup_namespace.get_symbol("__rust_realloc").upgrade().ok_or("couldn't get __rust_realloc symbol")?;
	// let parent_crate_ref = rust_alloc.lock().parent_crate.upgrade().ok_or("couldn't get alloc symbols' parent_crate")?;
	// let parent_crate = parent_crate_ref.lock_as_ref();
	// let symbols_to_add = vec![rust_alloc, rust_dealloc, rust_oom, rust_realloc];
	// debug!("Adding symbols to parent_crate {:?}: \n{:?}", parent_crate.deref(), symbols_to_add);
	// simd_namespace.add_symbols(symbols_to_add.iter(), true);
	// simd_namespace.crate_tree.lock().insert(String::from("nano_core_alloc"), parent_crate_ref.clone());


	let new_modules = vec![compiler_builtins_simd, rlibc_simd, core_lib_simd, panic_unwind_simd];
	// simd_namespace.load_kernel_crates(new_modules.into_iter(), None, kernel_mmi_ref.lock().deref_mut(), true)?;
	simd_namespace.load_kernel_crates(new_modules.into_iter(), Some(backup_namespace), kernel_mmi_ref.lock().deref_mut(), false)?;

	let simd_test = get_module("k_sse#simd_test").ok_or("couldn't get module k_sse#simd_test")?;
	simd_namespace.load_kernel_crate(simd_test, None, kernel_mmi_ref.lock().deref_mut(), false)?;

	
	type SimdTestFunc = fn(());
	let section_ref1 = simd_namespace.get_symbol_or_load("simd_test::test1", SSE_KERNEL_PREFIX, None, kernel_mmi_ref.lock().deref_mut(), false)
		.upgrade()
		.ok_or("no symbol: simd_test::test1")?;
	let mut space1 = 0;	
	let (mapped_pages1, mapped_pages_offset1) = { 
		let section = section_ref1.lock();
		(section.mapped_pages.clone(), section.mapped_pages_offset)
	};
	let func1: &SimdTestFunc = mapped_pages1.lock().as_func(mapped_pages_offset1, &mut space1)?;
	spawn::spawn_kthread(func1, (), String::from("simd_test_1-sse"), None)?;
	debug!("finished spawning first simd task");


	let section_ref2 = simd_namespace.get_symbol_or_load("simd_test::test2", SSE_KERNEL_PREFIX, None, kernel_mmi_ref.lock().deref_mut(), false)
		.upgrade()
		.ok_or("no symbol: simd_test::test2")?;
	let mut space2 = 0;	
	let (mapped_pages2, mapped_pages_offset2) = { 
		let section = section_ref2.lock();
		(section.mapped_pages.clone(), section.mapped_pages_offset)
	};
	let func: &SimdTestFunc = mapped_pages2.lock().as_func(mapped_pages_offset2, &mut space2)?;
	spawn::spawn_kthread(func, (), String::from("simd_test_2-sse"), None)?;
	debug!("finished spawning second simd task");


	let section_ref3 = simd_namespace.get_symbol_or_load("simd_test::test3", SSE_KERNEL_PREFIX, None, kernel_mmi_ref.lock().deref_mut(), false)
		.upgrade()
		.ok_or("no symbol: simd_test::test3")?;
	let mut space3 = 0;	
	let (mapped_pages3, mapped_pages_offset3) = { 
		let section = section_ref3.lock();
		(section.mapped_pages.clone(), section.mapped_pages_offset)
	};
	let func: &SimdTestFunc = mapped_pages3.lock().as_func(mapped_pages_offset3, &mut space3)?;
	spawn::spawn_kthread(func, (), String::from("simd_test_3-sse"), None)?;
	debug!("finished spawning third simd task");


	loop {

	}

	// Err("unfinished")
}
