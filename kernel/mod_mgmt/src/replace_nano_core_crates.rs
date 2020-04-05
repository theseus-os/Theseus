//! Routines for replacing the crates that comprise the `nano_core`,
//! i.e., the base kernel image. 
//! 
//! In loadable mode, this should be invoked to duplicitously load each one 
//! of the nano_core's constituent crates such that other crates loaded in the future
//! will depend on those dynamically-loaded instances rather than 
//! the statically-linked sections in the nano_core's base kernel image.
//!
//! As of now, the newly-loaded crates will still depend on (and link against)
//! the data sections (.data and .bss) in the actual nano_core's memory pages. 
//! I haven't found a correct, safe way to replace those data sections yet,
//! since the type contract of a static variable is that is a singleton instance for all eternity.
//! 
//! However, the non-data sections **are** replaced, e.g., .text, .rodata. 
//! All crates loaded in the future will depend on those newly-loaded (non-data) sections 
//! instead of their original counterparts in the nano_core.
//! 



use super::{CrateNamespace, LoadedCrate, StrongCrateRef, StrongSectionRef, SectionType, MmiRef};
use alloc::{
    collections::{BTreeSet, BTreeMap},
    string::String,
    sync::Arc,
};
use fs_node::FileRef;


/// See the module-level documentation for how this works. 
/// 
/// # Current Limitations
/// Currently, this does not actually rewrite the existing sections in the nano_core itself
/// to redirect them to depend on the newly-loaded crate instances. 
/// All it can do is replace the existing nano_core
/// This is a limitation that we're working to overcome, and is difficult because we need
/// to obtain a list of all linker relocations that were performed by the static linker
/// such that we can properly rewrite the existing dependencies. 
/// For simple relocations like RX86_64_64 where the value is just an absolute address,
/// it would be straightforward to simply scan the nano_core's `kernel.bin` binary file 
/// for each section's virtual address in order to derive/recover the relocations and dependencies between sections,
/// but it's not always that simple for other relocation types.
/// Perhaps we can ask the linker to emit a list of relocations it performs and then provide 
/// that list as input into this function so that it doesn't have to guess when deriving relocations.
pub fn replace_nano_core_crates(
    namespace: &Arc<CrateNamespace>,
    nano_core_crate_ref: StrongCrateRef,
    kernel_mmi_ref: &MmiRef,
) -> Result<(), &'static str> {
    let mut constituent_crates = BTreeSet::new();
    for sec_name in nano_core_crate_ref.lock_as_ref().global_symbols.iter() {
        for n in super::get_containing_crate_name(sec_name.as_str()) {
            constituent_crates.insert(String::from(n));
        }
    }

    let nano_core_crate = nano_core_crate_ref.lock_as_ref();

    // As a first attempt, let's try to load and replace the "memory" crate
    let (crate_object_file, _ns) = CrateNamespace::get_crate_object_file_starting_with(namespace, "memory-")
        .ok_or("Failed to find \"memory-\" crate")?;

    let _new_crate_ref = load_crate_using_nano_core_data_sections(
        &nano_core_crate, 
        &namespace,
        &crate_object_file,
        kernel_mmi_ref,
        false,
    )?;

    debug!("Replaced nano_core constituent crate: {:?}", _new_crate_ref);

    Ok(())
}



/// Load the given crate, but instead of using its own data sections (.data and .bss),
/// we use the corresponding sections from the nano_core base image. 
/// This avoids the problem of trying to create a second instace of a static variable
/// that's supposed to be a system-wide singleton. 
/// We could technically achieve it by forcing every static variable to be `Clone`, 
/// e.g., by wrapping it in an `Arc`, but we cannot do this for early crates like `memory`
/// that exist before we have the ability to use alloc types.
/// 
/// Instead of calling load_crate() and loading the new crate as normal, we should break it down into its constituent parts. 
/// (1) Call load_crate_sections as normal. The data sections will be ignored, but that's okay.
/// (2) Go through the .data/.bss sections in the partially loaded new crate, and replace them with references to
///     the corresponding data section in the nano_core.
///     We must replace both the sections in the `sections` map and in the `data_sections` map.
///     (Check that the sections are actually being dropped by instrumenting `LoadedSection::Drop`.)
/// (3) Call perform_relocations as normal, which will use the changed data/bss sections
/// (4) Take and drop the new crate's `data_pages`, just to ensure it's not actually being used. 
///     Check that the data pages are getting unmapped in MappedPages::unmap()  (e.g., set a breakpoint using GDB?)
fn load_crate_using_nano_core_data_sections(
    nano_core_crate: &LoadedCrate,
    namespace: &Arc<CrateNamespace>,
    crate_object_file: &FileRef,
    kernel_mmi_ref: &MmiRef, 
    verbose_log: bool,
) -> Result<StrongCrateRef, &'static str> {

    let cf = crate_object_file.lock();

    // (1) Load the crate's sections. We won't use the .data/.bss sections, but that's fine.
    let (new_crate_ref, elf_file) = namespace.load_crate_sections(&*cf, kernel_mmi_ref, verbose_log)?;

    // (2) Go through the .data/.bss sections in the partially loaded new crate,
    //     and replace them with references to the corresponding data section in the nano_core.
    //     We must replace both the sections in the `sections` map and in the `data_sections` map.
    //     (Check that the sections are actually being dropped by instrumenting `LoadedSection::Drop`.)
    // 
    // In the meantime, populate a map of newly-loaded section index to the existing old data section in the nano_core that matches it.
    // Note that the newly-loaded section index itself already maps to the newly-loaded data section, in the `new_crate_ref.sections` map.
    let _map_new_sec_shndx_to_old_sec_in_nano_core: BTreeMap<usize, StrongSectionRef> = {
        let mut map = BTreeMap::new();
        // Get an iterator of all data sections in the newly-loaded crate and their shndx values.
        // We can't use the `data_sections` field in the `LoadedCrate` struct because it doesn't have shndx values for each section.
        let mut new_crate = new_crate_ref.lock_as_mut().ok_or_else(|| "BUG: could not get exclusive mutable access to newly-loaded crate")?;
        let new_crate_sections_mut = new_crate.sections.iter_mut()
            .filter(|(_shndx, sec)| sec.get_type() == SectionType::Data || sec.get_type() == SectionType::Bss);
        
        for (shndx, new_sec) in new_crate_sections_mut {
            // Get the section from the nano_core that exactly matches the new_sec.
            let old_sec = nano_core_crate.data_sections.get_str(&new_sec.name).ok_or_else(|| {
                error!("BUG: couldn't find old_sec in nano_core to copy data into new_sec {:?} (.data/.bss state transfer)", new_sec.name);
                "BUG: couldn't find old_sec in nano_core to copy data into new_sec (.data/.bss state transfer)"
            })?;
            map.insert(*shndx, old_sec.clone());

            // replace the new crate's data section references with references to the corresponding sections in the nano_core
            *new_sec = old_sec.clone();
        }

        // do the same section replacement in the new_crate's list of `data_sections`
        for new_data_sec in new_crate.data_sections.values_mut() {
            let old_sec = map.values()
                .find(|s| s.name == new_data_sec.name)
                .ok_or("BUG: couldn't find matching data_section in new_crate")?;
            *new_data_sec = old_sec.clone();
        }

        map

    };

    // debug!("New crate data sections: {:?}", map_new_sec_shndx_to_old_sec_in_nano_core);

    // (3) Perform the actual relocations, using the replaced data sections above.
    namespace.perform_relocations(&elf_file, &new_crate_ref, None, kernel_mmi_ref, verbose_log)?;
    
    // (4) Remove and drop the new_crate's data MappedPages, since it shouldn't be used. 

    // Lastly, do all the final parts of loading a crate:
    // adding its symbols to the map, and inserting it into the crate tree.
    // We only need to add non-data sections, since the data sections that we're using from the nano_core
    // have already been added to the symbol map when the nano_core was originally parsed.
    let (new_crate_name, num_sections, new_syms) = {
        let new_crate = new_crate_ref.lock_as_ref();
        let new_syms = namespace.add_symbols_filtered(
            new_crate.sections.values(), 
            |sec| sec.get_type() != SectionType::Data && sec.get_type() != SectionType::Bss,
            verbose_log,
        );
        (new_crate.crate_name.clone(), new_crate.sections.len(), new_syms)
    };
        
    debug!("loaded new crate {:?}, num sections: {}, added {} new symbols.", new_crate_name, num_sections, new_syms);
    namespace.crate_tree.lock().insert(new_crate_name.into(), new_crate_ref.clone_shallow());
    Ok(new_crate_ref)
}
