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
//! # Context Switching 
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
//! # Interrupt Handling
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

// NOTE: the `cfg_if` macro makes the entire file dependent upon the `simd_personality` config.
#[macro_use] extern crate cfg_if;
cfg_if! { if #[cfg(simd_personality)] {


/* 
 * NOTE: now, we're using the compiler_builtins crate that is built by cargo's build-std feature by default, but we can switch back
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
extern crate apic;
extern crate fs_node;


use alloc::{
	string::String,
	sync::Arc,
};
use mod_mgmt::{CrateNamespace, CrateType, NamespaceDir, get_initial_kernel_namespace, get_namespaces_directory};
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
	let namespace_prefix = match simd_ext {
		SimdExt::AVX => "avx",
		SimdExt::SSE => "sse",
		SimdExt::None => return Err("Cannot create a new SIMD personality with SimdExt::None!"),
	};

	let kernel_mmi_ref = memory::get_kernel_mmi_ref().ok_or_else(|| "couldn't get kernel mmi")?;
	let backup_namespace = get_initial_kernel_namespace().ok_or("initial kernel crate namespace wasn't yet initialized")?;

	// The `mod_mgmt::init()` function should have initialized the following directories, 
	// for example, if 'sse' was the prefix used to build the SSE versions of each crate:
	//     .../namespaces/sse_kernel
	//     .../namespaces/sse_application
	let namespaces_dir = get_namespaces_directory().ok_or("top-level namespaces directory wasn't yet initialized")?;
	
	// Create the SIMD kernel namespace
	let simd_kernel_namespace = {
		let namespace_name = format!("{}{}", namespace_prefix, CrateType::Kernel.default_namespace_name());
		let dir = namespaces_dir.lock().get_dir(&namespace_name).ok_or("couldn't find SIMD kernel namespace directory at given path")?;
		Arc::new(CrateNamespace::new(
			String::from(namespace_name), 
			NamespaceDir::new(dir),
			None,
		))
	};

	// Then create the SIMD application namespace that is recursively backed by the simd_kernel_namespace
	let mut simd_app_namespace = {
		let namespace_name = format!("{}{}", namespace_prefix, CrateType::Application.default_namespace_name());
		let dir = namespaces_dir.lock().get_dir(&namespace_name).ok_or("couldn't find SIMD application namespace directory at given path")?;
		CrateNamespace::new(
			String::from(namespace_name), 
			NamespaceDir::new(dir),
			Some(Arc::clone(&simd_kernel_namespace)),
		)
	};

	// Load things that are specific (private) to the SIMD world, like core library and compiler builtins
	let (compiler_builtins_simd, _ns) = CrateNamespace::get_crate_object_file_starting_with(&simd_kernel_namespace, "compiler_builtins-")
		.ok_or_else(|| "couldn't find a single 'compiler_builtins' object file in simd_personality")?;
	let (core_lib_simd, _ns) = CrateNamespace::get_crate_object_file_starting_with(&simd_kernel_namespace, "core-")
		.ok_or_else(|| "couldn't find a single 'core' object file in simd_personality")?;
	let crate_files = [compiler_builtins_simd, core_lib_simd];
	simd_kernel_namespace.load_crates(crate_files.iter(), Some(backup_namespace), &kernel_mmi_ref, false)?;
	

	// load the actual crate that we want to run in the simd namespace, "simd_test"
	let (simd_test_file, _ns) = simd_app_namespace.method_get_crate_object_file_starting_with("simd_test-")
		.ok_or_else(|| "couldn't find a single 'simd_test' object file in simd_personality")?;
	simd_app_namespace.enable_fuzzy_symbol_matching();
	simd_app_namespace.load_crate(&simd_test_file, Some(backup_namespace), &kernel_mmi_ref, false)?;
	simd_app_namespace.disable_fuzzy_symbol_matching();


	let this_core = apic::get_my_apic_id();
	
	type SimdTestFunc = fn(());
	let section_ref1 = simd_app_namespace.get_symbol_starting_with("simd_test::test1::")
		.upgrade()
		.ok_or("no single symbol matching \"simd_test::test1\"")?;
	let func1: &SimdTestFunc = unsafe { section_ref1.as_func() }?;
	let _task1 = spawn::new_task_builder(*func1, ())
		.name(format!("simd_test_1-{}", simd_app_namespace.name()))
		.pin_on_core(this_core)
		.simd(simd_ext)
		.spawn()?;
	debug!("finished spawning simd_test::test1 task");


	let section_ref2 = simd_app_namespace.get_symbol_starting_with("simd_test::test2::")
		.upgrade()
		.ok_or("no single symbol matching \"simd_test::test2\"")?;
	let func2: &SimdTestFunc = unsafe { section_ref2.as_func() }?;
	let _task2 = spawn::new_task_builder(*func2, ())
		.name(format!("simd_test_2-{}", simd_app_namespace.name()))
		.pin_on_core(this_core)
		.simd(simd_ext)
		.spawn()?;
	debug!("finished spawning simd_test::test2 task");


	let section_ref3 = simd_app_namespace.get_symbol_starting_with("simd_test::test_short::")
		.upgrade()
		.ok_or("no single symbol matching \"simd_test::test_short\"")?;
	let func3: &SimdTestFunc = unsafe { section_ref3.as_func() }?;
	let _task3 = spawn::new_task_builder(*func3, ())
		.name(format!("simd_test_short-{}", simd_app_namespace.name()))
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
	
	// _task1.join()?;
	// _task2.join()?;
	// _task3.join()?;
	// Ok(())
}
		
}} // end of cfg_if block
