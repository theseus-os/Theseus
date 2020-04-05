//! Routines for replacing the crates that comprise the `nano_core`,
//! i.e., the base kernel image. 
//! 
//! In loadable mode, this should be invoked to duplicitously load each one 
//! of the nano_core's constituent crates such that other crates loaded in the future
//! will depend on those dynamically-loaded instances rather than 
//! the statically-linked sections in the nano_core's base kernel image.


use super::{CrateNamespace, StrongCrateRef, StrongSectionRef, SectionType, MmiRef};
use alloc::{
    collections::{BTreeSet, BTreeMap},
    string::String,
    sync::Arc,
};


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

    // As a first attempt, let's try to load and replace the "memory" crate
    let (crate_object_file, _ns) = CrateNamespace::get_crate_object_file_starting_with(namespace, "memory-")
        .ok_or("Failed to find \"memory-\" crate")?;

    let (new_crate_ref, _new_syms) = namespace.load_crate(&crate_object_file, None, kernel_mmi_ref, false)?;
    let nano_core_crate = nano_core_crate_ref.lock_as_ref();

    for new_sec in new_crate_ref.lock_as_ref().sections.values() {
        debug!("new_crate section: {:?}", new_sec);
    }

    // for new_sec in nano_core_crate.data_sections.values() {
    //     debug!("nano_core data section: {:?}", new_sec);
    // }

    
    // TODO FIXME 
    // TODO FIXME
    // TODO FIXME:  this doesn't work, it's not always correct to just byte-wise copy a .data/.bss section
    //              to another address, i presume because they have addresses that are relevant offsets
    //              which reference something in the old section (source) from the new section (dest). 
    //              Instead, I believe a proper solution is to require all static variables to be Clone,
    //              which we can implement with a type wrapper around Once/LazyStatic or something, 
    //              and eventually enforce it with a compiler plugin.
    //              Still not sure how to actually invoke the Clone::clone() method on 
    //              a generic static variable instance, since all we have here is a pointer to the section data. 
    //              But we can try it by hardcoding a couple test cases, e.g., for KERNEL_MMI or FRAME_ALLOCATOR.


    // IDEA:  something we could try here is to go through all of the new_crate's data sections, 
    //        and then go through each sections' list of weak dependents in order to figure out which other sections depend on them. 
    //        Then we could rewrite those dependencies' relocation entries to point to the original nano_core instance instead.
    //        Obviously this is only needed for .data/.bss sections, and since you cannot recover those anyway
    //        then there's no point trying to move them to a different memory location. 
    //        Clearing & reclaiming the nano_core memory (especially .data/.bss sections that are in an initialized state)
    //        is going to be quite difficult, since we'd have to be 100% confident that nothing inside the nano_core
    //        or beyond the nano_core (any other crate) is still dependent upon those data sections.


    // NEW BETTER IDEA: 
    // Instead of calling load_crate() and loading the new crate as normal, we should break it down into its constituent parts. 
    // (1) Call load_crate_sections as normal. The data sections will be ignored, but that's okay.
    // (2) Go through the .data/.bss sections in the partially loaded new crate, and replace them with references to
    //     the corresponding data section in the nano_core.
    //     We must replace both the sections in the `sections` map and in the `data_sections` map.
    //     (Check that the sections are actually being dropped by instrumenting `LoadedSection::Drop`.)
    // (3) Call perform_relocations as normal, which will use the changed data/bss sections
    // (4) Take and drop the new crate's `data_pages`, just to ensure it's not actually being used. 
    //     Check that the data pages are getting unmapped in MappedPages::unmap()  (e.g., set a breakpoint using GDB?)


    // First, populate a map of newly-loaded section index to the existing old data section in the nano_core that matches it.
    // Note that the newly-loaded section index itself already maps to the newly-loaded data section, in the `new_crate_ref.sections` map.
    let map_new_sec_shndx_to_old_sec_in_nano_core: BTreeMap<usize, StrongSectionRef> = {
        let mut map = BTreeMap::new();

        // Get an iterator of all data sections in the newly-loaded crate and their shndx values.
        // We can't use the `data_sections` field in the `LoadedCrate` struct because it doesn't have shndx values for each section.
        let new_crate = new_crate_ref.lock_as_ref();
        let iter = new_crate.sections.iter()
            .filter(|(_shndx, sec)| sec.get_type() == SectionType::Data || sec.get_type() == SectionType::Bss)
            .map(|(shndx, sec)| (*shndx, sec.clone()));
        
        for (shndx, new_sec) in iter {
            // Get the section from the nano_core that exactly matches the new_sec.
            let old_sec = nano_core_crate.data_sections.get_str(&new_sec.name).ok_or_else(|| {
                error!("BUG: couldn't find old_sec in nano_core to copy data into new_sec {:?} (.data/.bss state transfer)", new_sec.name);
                "BUG: couldn't find old_sec in nano_core to copy data into new_sec (.data/.bss state transfer)"
            })?;
            map.insert(shndx, old_sec.clone());
        }
        map
    };

    debug!("New crate data sections: {:?}", map_new_sec_shndx_to_old_sec_in_nano_core);

    // Just like when swapping a crate, we need to go through all the `.data` and `.bss` sections
    // and copy over the section data from the existing nano_core's section into the newly-loaded instance of that same section.
    // This is because they represent static variables that would otherwise result in a loss of data.
    for newly_loaded_data_sec in new_crate_ref.lock_as_ref().data_sections.values() {
        info!("Newly-loaded section {:?} has weak dependents:", newly_loaded_data_sec);
        for weak_dep in newly_loaded_data_sec.inner.read().sections_dependent_on_me.iter() {
            trace!("\t{:?}", weak_dep.section.upgrade());

        }

        // The list of sections_dependent_on_me ONLY includes sections from foreign crates. 
        // Thus, for completeness, we also must include the local sections within this crate
        // that are dependent upon this crate's own .data sections.



        // // Get the section from the nano_core that exactly matches the new_sec.
        // let old_sec = nano_core_crate.data_sections.get_str(&new_sec.name).ok_or_else(|| {
        //     error!("BUG: couldn't find old_sec in nano_core to copy data into new_sec {:?} (.data/.bss state transfer)", new_sec.name);
        //     "BUG: couldn't find old_sec in nano_core to copy data into new_sec (.data/.bss state transfer)"
        // })?;
        // debug!("copying .data or .bss section from old {:?} to new {:?}", old_sec, new_sec);
        // old_sec.copy_section_data_to(&new_sec)?;
    }


    // TODO: don't forget to undo the symbol map replacements (the nano_core's data sections were replaced with the new_crate's data sections)

    Ok(())
}