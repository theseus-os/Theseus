use alloc::{Vec, BTreeMap};
use metadata::StrongCrateRef;
use super::{CrateNamespace, map_crate_module, PartiallyLoadedCrate};
use memory::{ModuleArea, MappedPages, MemoryManagementInfo};


/// Swaps crate modules.
/// 
/// In general, the strategy for replacing and old module `C` with a new module `C'` consists of three simple steps:
/// 1) Load the new replacement module `C'`.
/// 2) Set up new relocation entries that redirect all module's dependencies on the old module `C` to the new module `C'`.
/// 3) Remove module `C` and clean it up, e.g., removing its entries from the system map.
pub fn swap_crates(
	swap_pairs: Vec<(StrongCrateRef, &ModuleArea)>,
	backup_namespace: &CrateNamespace,
	kernel_mmi: &mut MemoryManagementInfo,
	verbose_log: bool,
) -> Result<(), &'static str> {
	// create a new CrateNamespace and load all of the new crate modules into it
	let swap_pairs = swap_pairs.into_iter();
	let module_iter = swap_pairs.clone().map(|pair| pair.1);
	let new_namespace = load_crates_in_new_namespace(module_iter, backup_namespace, kernel_mmi, verbose_log)?;

	for (crate_name, crate_ref) in new_namespace.crate_tree.lock().iter() {
		debug!("====================== Loaded new crate \"{}\"  ===========================", crate_name);
		let krate = crate_ref.read();
		krate.text_pages.as_ref().map(|tp|   debug!("    text_pages  : {:#X} ({} pages)", tp.start_address(), tp.size_in_pages()));
		krate.rodata_pages.as_ref().map(|rp| debug!("    rodata_pages: {:#X} ({} pages)", rp.start_address(), rp.size_in_pages()));
		krate.data_pages.as_ref().map(|dp|   debug!("    data_pages  : {:#X} ({} pages)\n\n", dp.start_address(), dp.size_in_pages()));
	}

	// Now that we have loaded all of the new modules into the new namepsace in isolation,
	// we simply need to remove all of the old crates
	// and fix up all of the relocations `WeakDependents` for each of the existing sections
	// that depend on the old crate that we're replacing here,
	// such that they refer to the new_module instead of the old_crate.
	for (old_crate, _new_module) in swap_pairs {
		let old = old_crate.read();
		debug!("====================== Replacing old crate \"{}\"  ===========================", old.crate_name);
		old.text_pages.as_ref().map(|tp|   debug!("    text_pages  : {:#X} ({} pages)", tp.start_address(), tp.size_in_pages()));
		old.rodata_pages.as_ref().map(|rp| debug!("    rodata_pages: {:#X} ({} pages)", rp.start_address(), rp.size_in_pages()));
		old.data_pages.as_ref().map(|dp|   debug!("    data_pages  : {:#X} ({} pages)\n\n", dp.start_address(), dp.size_in_pages()));

		for sec_ref in &old.sections {
			let sec = sec_ref.lock();
			if false {
				if !sec.sections_i_depend_on.is_empty() {
					debug!("    Section \"{}\": sections i depend on (strong dependencies):", sec.name);
					for strong_dep in &sec.sections_i_depend_on {
						debug!("        {}", strong_dep.section.lock().name);
					}
				}
			}
			if true {
				if !sec.sections_dependent_on_me.is_empty() {
					debug!("    Section \"{}\": sections dependent on me (weak dependents):", sec.name);
					for weak_dep in &sec.sections_dependent_on_me {
						if let Some(wds) = weak_dep.section.upgrade() {
							debug!("        {}", wds.lock().name);
						}
						else {
							debug!("        ERROR: weak dependent failed to upgrade()");
						}
					}
				}
			}
		}
	}


	Err("unfinished")
}



/// This function first loads all of the given crates into a new, separate namespace in isolation,
/// and only after *all* crates are loaded does it move on to linking/relocation calculations. 
/// This allows them to be linked against each other first, rather than to always fall back to
/// linking against existing symbols in this namespace, so this namespace serves as the `backup_namespace`. 
/// It is this isolated preloading of crate sections that allows us to create a package of crates
/// that are all new and can be swapped as a single unit. 
fn load_crates_in_new_namespace<'a, I>(
	new_modules: I,
	backup_namespace: &CrateNamespace,
	kernel_mmi: &mut MemoryManagementInfo,
	verbose_log: bool,
) -> Result<CrateNamespace, &'static str> 
	where I: Iterator<Item = &'a ModuleArea> + Clone 
{
	// first we map all of the crates' ModuleAreas
	let mappings = {
		let mut mappings: Vec<MappedPages> = Vec::new(); //Vec::with_capacity(len);
		for crate_module in new_modules.clone() {
			mappings.push(map_crate_module(crate_module, kernel_mmi)?);
		}
		mappings
	};

	// create a new empty namespace so we can add symbols to it before performing the relocations
	let new_namespace = CrateNamespace::new();
	let mut partially_loaded_crates: Vec<PartiallyLoadedCrate> = Vec::with_capacity(mappings.len()); 

	// first we do all of the section parsing and loading
	for (i, crate_module) in new_modules.clone().enumerate() {
		let temp_module_mapping = mappings.get(i).ok_or("Fatal logic error: mapped crate module successfully but couldn't retrieve mapping (WTF?)")?;
		let plc = new_namespace.load_crate_sections(temp_module_mapping, crate_module.size(), crate_module.name(), kernel_mmi, verbose_log)?;
		let _new_syms = new_namespace.add_symbols(plc.loaded_sections.values(), &plc.new_crate.read().crate_name.clone(), verbose_log);
		partially_loaded_crates.push(plc);
	}
	
	// then we do all of the relocations 
	for plc in partially_loaded_crates {
		let new_crate = new_namespace.perform_relocations(&plc.elf_file, plc.new_crate, plc.loaded_sections, Some(backup_namespace), kernel_mmi, verbose_log)?;
		let name = new_crate.read().crate_name.clone();
		new_namespace.crate_tree.lock().insert(name, new_crate);
	}

	Ok(new_namespace)
}
