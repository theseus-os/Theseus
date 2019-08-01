//! Management of two kernel personalities, one for SIMD-enabled code, and one for regular code. 
//! 
//! This crate is responsible for creating and managing multiple `CrateNamespace`s so that Theseus
//! can run two instances of code side-by-side, like library OS-style personalities. 
//! This crate itself is part of the regular non-SIMD world, not the SIMD world. 
//! 
//! There are two considerations here to ensure correctness by saving/restoring SIMD registers: 
//! how to deal with it for context switching, and also for interrupt handling.
//! Both of these policies are determined per-core as follows:
//! 
//! ### Context Switching 
//! * If there is one or zero SIMD-enabled `Task`s running on a given core, 
//!   then all `Task`s running on that core can use the standard, 
//!   non-SIMD-enabled context switching routine. 
//!   This is because there is no need to save and restore SIMD registers (e.g., xmm),
//!   as only a single `Task` will ever use them. 
//! * If there are multiple SIMD-enabled `Task`s running on a given core,
//!   then all of the SIMD `Task`s on that core must use the SIMD-enabled context switching routine,
//!   which is found in the `context_switch_sse` crate. 
//! 
//! Note that there is no danger in forcing all SIMD Tasks to always use the SIMD-enabled context switching routine,
//! it will always be correct to do so.
//! Each Task can have its own Context and context_switch routine, based on whether or not it uses SIMD instructions,
//! and that can be determined statically and independently for each task, without considering which other tasks are running. 
//! We don't really need personalities for that, since the `context_switch` routines are self-contained.  
//! However, that static policy misses out on the performance optimization of 
//! not having to save/restore SIMD registers when only a single SIMD Task is running on a given core.
//! 
//! ### Interrupt Handling
//! * If interrupt handlers are only ever compiled for the regular world, i.e., 
//!   no interrupt handlers exist that are compiled to use SIMD instructions,
//!   then we do not have to save/restore SIMD registers on an interrupt because
//!   we're guaranteed that no interrupt handling code can ever use (overwrite) SIMD registers. 
//!   Thus, even if there are some SIMD enabled tasks running on a given core, an interrupt handler need not save
//!   those SIMD registers if it cannot possibly ever touch them. 
//! 
//! * If interrupt handlers _are_ compiled for the SIMD world and use SIMD instructions in the handler 
//!   (or any function accessible from the interrupt handler), then they must (and obviously will) save SIMD registers.
//!   In fact, I don't believe it's possible to compile an interrupt handler that uses SIMD instructions but doesn't save SIMD registers
//!   (at least while we're stil using the special x86 interrupt calling convention to have LLVM do it for us).
//!   
//! Thus, the best option is just to require that any SIMD-enabled interrupt handlers must save all SIMD registers, 
//! which is a rule determined completely independently of which tasks are running on that core. 
//! In general, this is a good rule, because it's poor design to have an interrupt handler do a lot of work,
//! such as processing data in a way that would need SIMD instructions. 
//! Instead, those processing stages should be moved out of the interrupt handler and into a separate Task elsewhere,
//! i.e., a classic bottom-half/top-half design.
//!


#![no_std]
#![feature(compiler_builtins_lib)]

// NOTE: the `cfg_if` macro makes the entire file dependent upon the `simd_personality` config.
#[macro_use] extern crate cfg_if;
cfg_if! { if #[cfg(simd_personality)] {


/* 
 * NOTE: now, we're using the compiler_builtins crate that is built by xargo by default, but we can switch back
 * to this one if needed since it does export different symbols based on Cargo.toml feature choices.
// This crate is required for the SIMD environment,
// so we can resolve symbols that the core lib requires. 
#[cfg(target_feature = "sse2")]
extern crate compiler_builtins as _compiler_builtins; 
*/


#[macro_use] extern crate log;
#[macro_use] extern crate alloc;
extern crate memory;
extern crate mod_mgmt;
extern crate task;
extern crate spawn;
#[cfg(target_arch = "x86_64")]
extern crate apic;
extern crate fs_node;


use alloc::string::String;
use mod_mgmt::{CrateNamespace, get_default_namespace, get_namespaces_directory, NamespaceDirectorySet};
use spawn::KernelTaskBuilder;
use fs_node::FileOrDir; 
use task::SimdExt;


/// Initializes a new SIMD personality based on the provided `simd_ext` level of given SIMD extensions, e.g., SSE, AVX.
pub fn setup_simd_personality(simd_ext: SimdExt) -> Result<(), &'static str> {
	match internal_setup_simd_personality(simd_ext) {
		Ok(o) => {
			debug!("SIMD personality setup completed successfully.");
			Ok(o)
		}
		Err(e) => {
			error!("Error setting up SIMD personality: {}", e); 
			Err(e)
		}
	}
}


fn internal_setup_simd_personality(simd_ext: SimdExt) -> Result<(), &'static str> {
	let namespace_name = match simd_ext {
		SimdExt::AVX => "avx",
		SimdExt::SSE => "sse",
		SimdExt::None => return Err("Cannot create a new SIMD personality with SimdExt::None!"),
	};

	let kernel_mmi_ref = memory::get_kernel_mmi_ref().ok_or_else(|| "couldn't get kernel mmi")?;
	let backup_namespace = get_default_namespace().ok_or("default crate namespace wasn't yet initialized")?;

	// The `mod_mgmt::init()` function should have initialized the following directories, 
	// for example, if 'sse' was the prefix used to build the SSE versions of each crate:
	//     .../namespaces/sse/
	//     .../namespaces/sse/kernel
	//     .../namespaces/sse/application
	//     .../namespaces/sse/userspace
	let namespaces_dir = get_namespaces_directory().ok_or("top-level namespaces directory wasn't yet initialized")?;
	let base_dir = match namespaces_dir.lock().get(namespace_name) {
		Some(FileOrDir::Dir(d)) => d,
		_ => return Err("couldn't find directory at given path"),
	};
	let mut simd_namespace = CrateNamespace::new(
		String::from(namespace_name), 
		NamespaceDirectorySet::from_existing_base_dir(base_dir).map_err(|e| {
			error!("Couldn't find expected namespace directory {:?}, did you choose the correct SimdExt?", namespace_name);
			e
		})?,
	);

	// Load things that are specific (private) to the SIMD world, like core library and compiler builtins
	let compiler_builtins_simd = simd_namespace.get_kernel_file_starting_with("compiler_builtins-")
		.ok_or_else(|| "couldn't find a single 'compiler_builtins' object file in simd_personality")?;
	let core_lib_simd = simd_namespace.get_kernel_file_starting_with("core-")
		.ok_or_else(|| "couldn't find a single 'core' object file in simd_personality")?;
	let crate_files = vec![compiler_builtins_simd, core_lib_simd];
	simd_namespace.load_kernel_crates(crate_files.iter(), Some(backup_namespace), &kernel_mmi_ref, false)?;


	// load the actual crate that we want to run in the simd namespace, "simd_test"
	let simd_test_file = simd_namespace.get_kernel_file_starting_with("simd_test-")
		.ok_or_else(|| "couldn't find a single 'simd_test' object file in simd_personality")?;
	simd_namespace.enable_fuzzy_symbol_matching();
	simd_namespace.load_kernel_crate(&simd_test_file, Some(backup_namespace), &kernel_mmi_ref, false)?;
	simd_namespace.disable_fuzzy_symbol_matching();


	let this_core = apic::get_my_apic_id().ok_or("couldn't get my APIC id")?;
	
	type SimdTestFunc = fn(());
	let section_ref1 = simd_namespace.get_symbol_starting_with("simd_test::test1::")
		.upgrade()
		.ok_or("no single symbol matching \"simd_test::test1\"")?;
	let mut space1 = 0;	
	let (mapped_pages1, mapped_pages_offset1) = { 
		let section = section_ref1.lock();
		(section.mapped_pages.clone(), section.mapped_pages_offset)
	};
	let func1: &SimdTestFunc = mapped_pages1.lock().as_func(mapped_pages_offset1, &mut space1)?;
	let task1 = KernelTaskBuilder::new(func1, ())
		.name(format!("simd_test_1-{}", namespace_name))
		.pin_on_core(this_core)
		.simd(simd_ext)
		.spawn()?;
	debug!("finished spawning simd_test::test1 task");


	let section_ref2 = simd_namespace.get_symbol_starting_with("simd_test::test2::")
		.upgrade()
		.ok_or("no single symbol matching \"simd_test::test2\"")?;
	let mut space2 = 0;	
	let (mapped_pages2, mapped_pages_offset2) = { 
		let section = section_ref2.lock();
		(section.mapped_pages.clone(), section.mapped_pages_offset)
	};
	let func: &SimdTestFunc = mapped_pages2.lock().as_func(mapped_pages_offset2, &mut space2)?;
	let task2 = KernelTaskBuilder::new(func, ())
		.name(format!("simd_test_2-{}", namespace_name))
		.pin_on_core(this_core)
		.simd(simd_ext)
		.spawn()?;
	debug!("finished spawning simd_test::test2 task");


	let section_ref3 = simd_namespace.get_symbol_starting_with("simd_test::test_short::")
		.upgrade()
		.ok_or("no single symbol matching \"simd_test::test_short\"")?;
	let mut space3 = 0;	
	let (mapped_pages3, mapped_pages_offset3) = { 
		let section = section_ref3.lock();
		(section.mapped_pages.clone(), section.mapped_pages_offset)
	};
	let func: &SimdTestFunc = mapped_pages3.lock().as_func(mapped_pages_offset3, &mut space3)?;
	let task3 = KernelTaskBuilder::new(func, ())
		.name(format!("simd_test_short-{}", namespace_name))
		.pin_on_core(this_core)
		.simd(simd_ext)
		.spawn()?;
	debug!("finished spawning simd_test::test_short task");


	// we can't return here because the mapped pages that contain
	// the simd_test functions being run must not be dropped 
	// until the threads are completed.
	// TODO FIXME: check for this somehow in the thread spawn code, perhaps by giving the new thread ownership of the MappedPages,
	//             just like we do for application Tasks

	loop { }
	
	task1.join()?;
	task2.join()?;
	task3.join()?;

	Ok(())
}
		
}} // end of cfg_if block
