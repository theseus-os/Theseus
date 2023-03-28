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


use super::{CrateNamespace, LoadedCrate, StrongCrateRef, MmiRef};
use alloc::{
    collections::BTreeSet,
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
    let nano_core_crate = nano_core_crate_ref.lock_as_ref();

    let mut constituent_crates = BTreeSet::new();
    for sec in nano_core_crate.global_sections_iter() {
        for n in super::get_containing_crate_name(sec.name.as_str()) {
            constituent_crates.insert(String::from(n));
        }
    }

    // For now we just replace the "memory" crate. 
    // More crates can be added later, up to every `constituent_crate` in the nano_core.
    let (crate_object_file, _ns) = CrateNamespace::get_crate_object_file_starting_with(namespace, "memory-")
        .ok_or("Failed to find \"memory-\" crate")?;

    let _new_crate_ref = load_crate_using_nano_core_data_sections(
        &nano_core_crate, 
        namespace,
        &crate_object_file,
        kernel_mmi_ref,
        false,
    )?;

    Ok(())
}


/// Load the given crate, but instead of using its own data sections (.data and .bss),
/// we use the corresponding sections from the nano_core base image. 
/// This avoids the problem of trying to create a second instace of a static variable
/// that's supposed to be a system-wide singleton. 
/// 
/// We could technically achieve it by forcing every static variable to be `Clone`, 
/// e.g., by wrapping it in an `Arc`, but we cannot do this for early crates like `memory`
/// that exist before we have the ability to use alloc types.
/// Note that we should do something like this for sharing static variables 
/// across different `CrateNamespace` personalities. 
/// 
/// # Procedure
/// Instead of calling load_crate() and loading the new crate as normal, we break it down into its constituent parts. 
/// 1. Call `load_crate_sections()` as normal. The data sections will be ignored, but that's okay.
/// 2. Go through the .data/.bss sections in the partially loaded new crate, and replace them with references to
///    the corresponding data section in the nano_core.
/// 3. Remove (take) and drop the new crate's `data_pages`, just to ensure they're not actually being used. 
/// 4. Add the new crate's global sections to the symbol map, as usual. 
///    We can skip adding data/bss sections to the symbol map since they were already added in `parse_nano_core()`.
/// 5. Call `perform_relocations()` as usual, which will use the changed data/bss sections above,
///    resulting in future crates depending on the nano_core's data sections as needed.
/// 6. Add the new crate into the `namespace`'s new crate tree, as usual.
fn load_crate_using_nano_core_data_sections(
    nano_core_crate: &LoadedCrate,
    namespace: &Arc<CrateNamespace>,
    crate_object_file: &FileRef,
    kernel_mmi_ref: &MmiRef, 
    verbose_log: bool,
) -> Result<StrongCrateRef, &'static str> {
    let cf = crate_object_file.lock();
    debug!("Replacing nano_core's constituent crate {:?}", cf.get_name());

    // (1) Load the crate's sections. We won't end up using the newly-loaded .data/.bss sections, but that's fine.
    let (new_crate_ref, elf_file) = namespace.load_crate_sections(&*cf, kernel_mmi_ref, verbose_log)?;

    let new_crate_name; 
    let _num_new_syms: usize;
    let _num_new_sections: usize;

    // (2) Go through the .data/.bss sections in the partially loaded new crate,
    //     and replace them with references to the corresponding data section in the nano_core.
    {
        let mut new_crate = new_crate_ref.lock_as_mut().ok_or("BUG: could not get exclusive mutable access to newly-loaded crate")?;

        for shndx in new_crate.data_sections.clone() {
            let new_data_sec = new_crate.sections.get_mut(&shndx).ok_or_else(|| {
                error!("BUG: new_crate's data section shndx {} wasn't in the new_crate's sections map.", shndx);
                "BUG: new_crate's data section shndx wasn't in the new_crate's sections map."
            })?;
            // Get the section from the nano_core that exactly matches the new_sec.
            let old_data_sec = nano_core_crate.data_sections_iter()
                .find(|&sec| sec.name == new_data_sec.name)
                .ok_or_else(|| {
                    error!("BUG: couldn't find old_data_sec in nano_core to copy data into new_data_sec {:?} (.data/.bss state transfer)", new_data_sec.name);
                    "BUG: couldn't find old_data_sec in nano_core to copy data into new_data_sec (.data/.bss state transfer)"
                })?;

            // Replace the data section references in the new crate's `sections` list with the corresponding sections in the nano_core
            *new_data_sec = Arc::clone(old_data_sec);
        }
        
        // (3) Remove and drop the new_crate's data MappedPages, since it shouldn't be used. 
        let _unused_new_data_pages = new_crate.data_pages.take();
        drop(_unused_new_data_pages);

        // (4) Just like in the regular load_crate() function, we add the new crate's symbols to the map. 
        // We only need to add non-data sections, since the data sections that we're using from the nano_core
        // have already been added to the symbol map when the nano_core was originally parsed.
        _num_new_syms = namespace.add_symbols_filtered(
            new_crate.sections.values(), 
            |sec| !sec.typ.is_data_or_bss(),
            verbose_log,
        );
        _num_new_sections = new_crate.sections.len();
        new_crate_name = new_crate.crate_name.clone();
    }

    // (5) Perform the actual relocations, using the replaced data sections above.
    namespace.perform_relocations(&elf_file, &new_crate_ref, None, kernel_mmi_ref, verbose_log)?;

    info!("Replaced nano_core constituent crate {:?}, num sections: {}, added {} new symbols (should be 0).",
        new_crate_name, _num_new_sections, _num_new_syms
    );
    // (6) Add the newly-loaded crate to the namespace.
    namespace.crate_tree.lock().insert(new_crate_name, new_crate_ref.clone_shallow());
    Ok(new_crate_ref)
}
