//! Defines functions and types for crate swapping, used in live evolution and fault recovery.
//! 

#![no_std]
#![cfg_attr(loscd_eval, allow(unused_assignments, unused_variables))]

#[macro_use] extern crate alloc;
#[macro_use] extern crate log;
#[macro_use] extern crate lazy_static;
extern crate spin; 
extern crate memory;
extern crate hashbrown;
extern crate mod_mgmt;
extern crate fs_node;
extern crate qp_trie;
extern crate path;
extern crate by_address;

#[cfg(loscd_eval)]
extern crate hpet;

use core::{
    fmt,
    ops::Deref,
};
use spin::Mutex;
use alloc::{
    borrow::Cow,
    collections::BTreeSet,
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};
use hashbrown::HashMap;
use memory::{EntryFlags, MmiRef};
use fs_node::{FsNode, FileOrDir, FileRef, DirRef};
use mod_mgmt::{
    CrateNamespace,
    NamespaceDir,
    IntoCrateObjectFile,
    write_relocation,
    crate_name_from_path,
    replace_containing_crate_name,
    StrongSectionRef,
    WeakDependent, StrRef,
};
use path::Path;
use by_address::ByAddress;


lazy_static! {
    /// The set of crates that have been previously unloaded (e.g., swapped out) from a `CrateNamespace`.
    /// These are kept in memory as a performance optimization, such that if 
    /// they are ever requested to be swapped in again, we can swap them back in 
    /// almost instantly by avoiding the expensive procedure of re-loading them into memory.
    /// 
    /// The unloaded cached crates are stored in the form of a CrateNamespace itself,
    /// such that we can easily query the cache for crates and symbols by name.
    /// 
    /// This is soft state that can be removed at any time with no effect on correctness.
    static ref UNLOADED_CRATE_CACHE: Mutex<HashMap<SwapRequestList, CrateNamespace>> = Mutex::new(HashMap::new());
}

/// Clears the cache of unloaded (swapped-out) crates saved from previous crate swapping operations. 
pub fn clear_unloaded_crate_cache() {
    UNLOADED_CRATE_CACHE.lock().clear();
}


/// A state transfer function is an arbitrary function called when swapping crates. 
/// 
/// See the `swap_crates()` function for more details. 
pub type StateTransferFunction = fn(&Arc<CrateNamespace>, &CrateNamespace) -> Result<(), &'static str>;


/// Swaps in new crates that can optionally replace existing crates in this `CrateNamespace`.
/// 
/// See the documentation of the [`SwapRequest`](#struct.SwapRequest.html) struct for more details.
/// 
/// In general, the strategy for replacing an old crate `C` with a new crate `C2` consists of three steps:
/// 1) Load the new replacement crate `C2` from its object file.
/// 2) Copy the .data and .bss sections from old crate `C` to the new crate `C2`
/// 3) Set up new relocation entries that redirect all dependencies on the old crate `C` to the new crate `C2`.
/// 4) Remove crate `C` and clean it up, e.g., removing its entries from the symbol map.
///    Save the removed crate (and its symbol subtrie) in a cache for later use to expedite future swapping operations.
/// 
/// The given `CrateNamespace` is used as the backup namespace for resolving unknown symbols,
/// in adddition to any recursive namespaces on which this namespace depends.
/// 
/// Upon a successful return, this namespace (and/or its recursive namespace) will have the new crates in place of the old ones,
/// and the old crates will be completely removed from this namespace. 
/// In this namespace there will be no remaining dependencies on the old crates, 
/// although crates in other namespaces may still include and depend upon those old crates.
/// 
/// # Arguments
/// * `swap_requests`: a list of several `SwapRequest`s, in order to allow swapping multiple crates all at once 
///   as a single "atomic" procedure, which prevents weird linking/relocation errors, 
///   such as a new crate linking against an old crate that already exists in this namespace
///   instead of linking against the new one that we want to replace that old crate with. 
/// * `override_namespace_dir`: the directories of object files from which missing crates should be loaded.
///   If a crate cannot be found in this directory set, this namespace's directory set will be searched for the crate. 
///   If `None`, only this `CrateNamespace`'s directory set (and its recursive namespace's) will be used to find missing crates to be loaded.
/// * `state_transfer_functions`: the fully-qualified symbol names of the state transfer functions, 
///   arbitrary functions that are invoked after all of the new crates are loaded but before the old crates are unloaded, 
///   in order to allow transfer of states from old crates to new crates or proper setup of states for the new crates.
///   These function should exist in the new namespace (or its directory set) and should take the form of a 
///   [`StateTransferFunction`](../type.StateTransferFunction.html).
///   The given functions are invoked with the arguments `(current_namespace, new_namespace)`, 
///   in which the `current_namespace` is the one currently running that contains the old crates,
///   and the `new_namespace` contains only newly-loaded crates that are not yet being used.
///   Both namespaces may (and likely will) contain more crates than just the old and new crates specified in the swap request list.
/// * `kernel_mmi_ref`: a reference to the kernel's `MemoryManagementInfo`.
/// * `verbose_log`: enable verbose logging.
/// 
/// # Warning: Correctness not guaranteed
/// This function currently makes no attempt to guarantee correct operation after a crate is swapped. 
/// For example, if the new crate changes a function or data structure, there is no guarantee that 
/// other crates will be swapped alongside that one. 
/// It will most likely error out, but this responsibility is currently left to the caller.
/// 
/// # Crate swapping optimizations
/// When one or more crates is swapped out, they are not fully unloaded, but rather saved in a cache
/// in order to accelerate future swapping commands. 
/// 
pub fn swap_crates(
    this_namespace: &Arc<CrateNamespace>,
    swap_requests: SwapRequestList,
    override_namespace_dir: Option<NamespaceDir>,
    state_transfer_functions: Vec<String>,
    kernel_mmi_ref: &MmiRef,
    verbose_log: bool,
    cache_old_crates: bool
) -> Result<(), &'static str> {

    #[cfg(not(loscd_eval))]
    debug!("swap_crates()[0]: \n\t-->override dir: {:?}, \n\t-->cache_old_crates: {:?}, \n\t-->state transfer: {:?},\n\t-->swap_requests: {:?}", 
        override_namespace_dir.as_ref().map(|d| d.lock().get_name()), 
        cache_old_crates,
        state_transfer_functions,
        swap_requests
    );

    #[cfg(loscd_eval)]
    let hpet = hpet::get_hpet().ok_or("couldn't get HPET timer")?;
    #[cfg(loscd_eval)]
    let hpet_start_swap = hpet.get_counter();
    
    let (namespace_of_new_crates, is_optimized) = {
        #[cfg(not(loscd_eval))] {
            // First, before we perform any expensive crate loading, let's try an optimization
            // based on cached crates that were unloaded during a previous swap operation. 
            if let Some(previously_cached_crates) = UNLOADED_CRATE_CACHE.lock().remove(&swap_requests) {
                warn!("Using optimized swap routine to swap in previously cached crates: {:?}", previously_cached_crates.crate_names(true));
                (previously_cached_crates, true)
            } else {
                // If no optimization is possible (no cached crates exist for this swap request), 
                // then create a new CrateNamespace and load all of the new crate modules into it from scratch.
                let nn = CrateNamespace::new(
                    String::from("temp_swap"), // format!("temp_swap--{:?}", swap_requests), 
                    // use the optionally-provided directory of crates instead of the current namespace's directories.
                    override_namespace_dir.unwrap_or_else(|| this_namespace.dir().clone()),
                    None,
                );
                // Note that we only need to load the crates that are replacing already-loaded old crates in the old namespace.
                let crate_file_iter = swap_requests.iter().filter_map(|swap_req| {
                    swap_req.old_crate_name.as_deref()
                        .and_then(|ocn| swap_req.old_namespace.get_crate(ocn))
                        .map(|_old_loaded_crate| swap_req.new_crate_object_file.deref())
                });
                nn.load_crates(crate_file_iter, Some(this_namespace), kernel_mmi_ref, verbose_log)?;
                (nn, false)
            }
        }
        #[cfg(loscd_eval)] {
            let nn = CrateNamespace::new(
                String::from("temp_swap"), // format!("temp_swap--{:?}", swap_requests), 
                // use the optionally-provided directory of crates instead of the current namespace's directories.
                override_namespace_dir.unwrap_or_else(|| this_namespace.dir().clone()),
                None,
            );
            // Note that we only need to load the crates that are replacing already-loaded old crates in the old namespace.
            let crate_file_iter = swap_requests.iter().filter_map(|swap_req| {
                swap_req.old_crate_name.as_deref()
                    .and_then(|ocn| swap_req.old_namespace.get_crate(ocn))
                    .map(|_old_loaded_crate| swap_req.new_crate_object_file.deref())
            });
            nn.load_crates(crate_file_iter, Some(this_namespace), kernel_mmi_ref, verbose_log)?;
            (nn, false)
        }
    };
        
    #[cfg(loscd_eval)]
    let hpet_after_load_crates = hpet.get_counter();

    #[cfg(not(loscd_eval))]
    let (mut future_swap_requests, cached_crates) = if cache_old_crates {
        (
            SwapRequestList::with_capacity(swap_requests.len()),
            CrateNamespace::new(
                format!("cached_crates--{:?}", swap_requests), 
                this_namespace.dir().clone(),
                None
            ),
        )
    } else {
        // When not caching old crates, these won't be used, so just make them empty dummy values.
        (SwapRequestList::new(), CrateNamespace::new(String::new(), this_namespace.dir().clone(), None))
    };


    #[cfg(loscd_eval)]
    let mut hpet_total_symbol_finding = 0;
    #[cfg(loscd_eval)]
    let mut hpet_total_rewriting_relocations = 0;
    #[cfg(loscd_eval)]
    let mut hpet_total_fixing_dependencies = 0;
    #[cfg(loscd_eval)]
    let mut hpet_total_bss_transfer = 0;

    // The name of the new crate in each swap request. There is one entry per swap request.
    let mut new_crate_names: Vec<String> = Vec::with_capacity(swap_requests.len());
    // Whether the old crate was actually loaded into the old namespace. There is one entry per swap request.
    let mut old_crates_are_loaded: Vec<bool> = Vec::with_capacity(swap_requests.len());

    // Now that we have loaded all of the new modules into the new namepsace in isolation,
    // we simply need to fix up all of the relocations `WeakDependents` for each of the existing sections
    // that depend on the old crate that we're replacing here,
    // such that they refer to the new_module instead of the old_crate.
    for req in &swap_requests {
        let SwapRequest { old_crate_name, old_namespace, new_crate_object_file, new_namespace: _new_ns, reexport_new_symbols_as_old } = req; 
        let reexport_new_symbols_as_old = *reexport_new_symbols_as_old;

        // Populate the list of new crate names for future usage.
        let new_crate_name = crate_name_from_path(&Path::new(new_crate_object_file.lock().get_name())).to_string();
        new_crate_names.push(new_crate_name.clone());

        // Get a reference to the old crate that is currently loaded into the `old_namespace`.
        let old_crate_ref = match old_crate_name.as_deref().and_then(|ocn| CrateNamespace::get_crate_and_namespace(old_namespace, ocn)) {
            Some((ocr, _ns)) => {
                old_crates_are_loaded.push(true);
                ocr
            }
            _ => {
                // If the `old_crate_name` was `None`, or the old crate wasn't found, that means it wasn't currently loaded. 
                // Therefore, we don't need to do any symbol dependency replacement. 
                // All we need to do is replace that old crate's object file in the old namespace's directory.
                if let Some(ref ocn) = old_crate_name {
                    #[cfg(not(loscd_eval))]
                    info!("swap_crates(): note: old crate {:?} was not currently loaded into old_namespace {:?}", ocn, old_namespace.name());
                }
                old_crates_are_loaded.push(false);
                continue; 
            }
        };
        let old_crate = old_crate_ref.lock_as_mut().ok_or_else(|| {
            error!("Unimplemented: swap_crates(), old_crate: {:?}, doesn't yet support deep copying shared crates to get a new exclusive mutable instance", old_crate_ref);
            "Unimplemented: swap_crates() doesn't yet support deep copying shared crates to get a new exclusive mutable instance"
        })?;

        let new_crate_ref = if is_optimized {
            debug!("swap_crates(): OPTIMIZED: looking for new crate {:?} in cache", new_crate_name);
            namespace_of_new_crates.get_crate(&new_crate_name)
                .ok_or_else(|| "BUG: swap_crates(): Couldn't get new crate from optimized cache")?
        } else {
            #[cfg(not(loscd_eval))]
            debug!("looking for newly-loaded crate {:?} in temp namespace", new_crate_name);
            namespace_of_new_crates.get_crate(&new_crate_name)
                .ok_or_else(|| "BUG: Couldn't get new crate that should've just been loaded into a new temporary namespace")?
        };

        // scope the lock on the `new_crate_ref`
        {
            let mut new_crate = new_crate_ref.lock_as_mut().ok_or_else(|| 
                "BUG: swap_crates(): new_crate was unexpectedly shared in another namespace (couldn't get as exclusively mutable)...?"
            )?;

            // currently we're always clearing out the new crate's reexports because we recalculate them every time
            new_crate.reexported_symbols.clear();

            let old_crate_name_without_hash = String::from(old_crate.crate_name_without_hash());
            let new_crate_name_without_hash = String::from(new_crate.crate_name_without_hash());
            let crates_have_same_name = old_crate_name_without_hash == new_crate_name_without_hash;

            #[cfg(loscd_eval)]
            let hpet_start_bss_transfer = hpet.get_counter();

            // Go through all the `.data` and `.bss` sections and copy over the old_sec into the new source_sec,
            // as they represent static variables that would otherwise result in a loss of data.
            for old_sec in old_crate.data_sections_iter() {
                let old_sec_name_without_hash = old_sec.name_without_hash();
                // get the section from the new crate that corresponds to the `old_sec`
                let prefix = if crates_have_same_name {
                    Cow::from(old_sec_name_without_hash)
                } else {
                    if let Some(s) = replace_containing_crate_name(old_sec_name_without_hash, &old_crate_name_without_hash, &new_crate_name_without_hash) {
                        Cow::from(s)
                    } else {
                        Cow::from(old_sec_name_without_hash)
                    }
                };
                let new_dest_sec = {
                    let mut iter = new_crate.data_sections_iter().filter(|sec| sec.name.starts_with(&*prefix));
                    iter.next()
                        .filter(|_| iter.next().is_none()) // ensure single element
                        .ok_or_else(|| 
                            "couldn't find destination section in new crate to copy old_sec's data into (.data/.bss state transfer)"
                        )
                }?;

                #[cfg(not(loscd_eval))]
                debug!("swap_crates(): copying .data or .bss section from old {:?} to new {:?}", &*old_sec, new_dest_sec);
                old_sec.copy_section_data_to(&new_dest_sec)?;
            }

            #[cfg(loscd_eval)] {
                let hpet_end_bss_transfer = hpet.get_counter();
                hpet_total_bss_transfer += hpet_end_bss_transfer - hpet_start_bss_transfer;
            }

            // We need to find all of the "weak dependents" (sections that depend on the sections in the old crate)
            // and replace them by rewriting their relocation entries to point to the corresponding new section in the new_crate.
            //
            // Note that we only need to iterate through sections from the old crate that are public/global,
            // i.e., those that were previously added to this namespace's symbol map,
            // because other crates could not possibly depend on non-public sections in the old crate.
            for old_sec in old_crate.global_sections_iter() {
                #[cfg(not(loscd_eval))]
                debug!("swap_crates(): looking for old_sec_name: {:?}", old_sec.name);

                let old_sec_ns = this_namespace.get_symbol_and_namespace(&old_sec.name)
                    .map(|(_weak_sec, ns)| ns)
                    .ok_or_else(|| {
                        error!("BUG: swap_crates(): couldn't get old crate's section: {:?}", old_sec.name);
                        "BUG: swap_crates(): couldn't get old crate's section"
                    })?;

                #[cfg(not(loscd_eval))]
                debug!("swap_crates(): old_sec_name: {:?}, old_sec: {:?}", old_sec.name, old_sec);
                let old_sec_name_without_hash = old_sec.name_without_hash();


                // This closure finds the section in the `new_crate` that corresponds to the given `old_sec` from the `old_crate`.
                // And, if enabled, it will reexport that new section under the same name as the `old_sec`.
                // We put this procedure in a closure because it's relatively expensive, allowing us to run it only when necessary.
                let find_corresponding_new_section = |new_crate_reexported_symbols: &mut BTreeSet<StrRef>| -> Result<StrongSectionRef, &'static str> {
                    // Use the new namespace to find the new source_sec that old target_sec should point to.
                    // The new source_sec must have the same name as the old one (old_sec here),
                    // otherwise it wouldn't be a valid swap -- the target_sec's parent crate should have also been swapped.
                    // The new namespace should already have that symbol available (i.e., we shouldn't have to load it on demand);
                    // if not, the swapping action was never going to work and we shouldn't go through with it.

                    // Find the section in the new crate that matches (or "fuzzily" matches) the current section from the old crate.
                    let new_crate_source_sec = if crates_have_same_name {
                        namespace_of_new_crates.get_symbol(&old_sec.name).upgrade()
                            .or_else(|| namespace_of_new_crates.get_symbol_starting_with(old_sec_name_without_hash).upgrade())
                    } else {
                        // here, the crates *don't* have the same name
                        if let Some(s) = replace_containing_crate_name(old_sec_name_without_hash, &old_crate_name_without_hash, &new_crate_name_without_hash) {
                            namespace_of_new_crates.get_symbol(&s).upgrade()
                                .or_else(|| namespace_of_new_crates.get_symbol_starting_with(&s).upgrade())
                        } else {
                            // same as the default case above (crates have same name)
                            namespace_of_new_crates.get_symbol(&old_sec.name).upgrade()
                                .or_else(|| namespace_of_new_crates.get_symbol_starting_with(old_sec_name_without_hash).upgrade())
                        }
                    }.ok_or_else(|| {
                        error!("swap_crates(): couldn't find section in the new crate that corresponds to a match of the old section {:?}", old_sec.name);
                        "couldn't find section in the new crate that corresponds to a match of the old section"
                    })?;
                    #[cfg(not(loscd_eval))]
                    debug!("swap_crates(): found match for old source_sec {:?}, new source_sec: {:?}", &*old_sec, &*new_crate_source_sec);

                    if reexport_new_symbols_as_old && old_sec.global {
                        // reexport the new source section under the old sec's name, i.e., redirect the old mapping to the new source sec
                        let reexported_name = old_sec.name.clone();
                        new_crate_reexported_symbols.insert(reexported_name.clone());
                        let _old_val = old_sec_ns.symbol_map().lock().insert(reexported_name, Arc::downgrade(&new_crate_source_sec));
                        if _old_val.is_none() { 
                            warn!("swap_crates(): reexported new crate section that replaces old section {:?}, but that old section unexpectedly didn't exist in the symbol map", old_sec.name);
                        }
                    }
                    Ok(new_crate_source_sec)
                };


                // the section from the `new_crate` that corresponds to the `old_sec` from the `old_crate`
                let mut new_sec: Option<StrongSectionRef> = None;

                // Iterate over all sections that depend on the old_sec. 
                let mut dead_weak_deps_to_remove: Vec<usize> = Vec::new();
                for (i, weak_dep) in old_sec.inner.read().sections_dependent_on_me.iter().enumerate() {
                    let target_sec = if let Some(sr) = weak_dep.section.upgrade() {
                        sr
                    } else {
                        #[cfg(not(any(loscd_eval, downtime_eval)))]
                        trace!("Removing dead weak dependency on old_sec: {}", old_sec.name);
                        dead_weak_deps_to_remove.push(i);
                        continue;
                    };
                    let relocation_entry = weak_dep.relocation;

                    #[cfg(loscd_eval)]
                    let start_symbol_finding = hpet.get_counter();
                    

                    // get the section from the new crate that corresponds to the `old_sec`
                    let new_source_sec = if let Some(ref nsr) = new_sec {
                        #[cfg(not(loscd_eval))]
                        trace!("using cached version of new source section");
                        nsr
                    } else {
                        #[cfg(not(loscd_eval))]
                        trace!("Finding new source section from scratch");
                        let nsr = find_corresponding_new_section(&mut new_crate.reexported_symbols)?;
                        new_sec.get_or_insert(nsr)
                    };

                    #[cfg(loscd_eval)] {
                        let end_symbol_finding = hpet.get_counter();
                        hpet_total_symbol_finding += end_symbol_finding - start_symbol_finding;
                    }

                    #[cfg(not(loscd_eval))]
                    debug!("    swap_crates(): target_sec: {:?}, old source sec: {:?}, new source sec: {:?}", &*target_sec, &*old_sec, &*new_source_sec);

                    // If the target_sec's mapped pages aren't writable (which is common in the case of swapping),
                    // then we need to temporarily remap them as writable here so we can fix up the target_sec's new relocation entry.
                    {
                        #[cfg(loscd_eval)]
                        let start_rewriting_relocations = hpet.get_counter();

                        let mut target_sec_mapped_pages = target_sec.mapped_pages.lock();
                        let target_sec_initial_flags = target_sec_mapped_pages.flags();
                        if !target_sec_initial_flags.is_writable() {
                            target_sec_mapped_pages.remap(&mut kernel_mmi_ref.lock().page_table, target_sec_initial_flags | EntryFlags::WRITABLE)?;
                        }

                        write_relocation(
                            relocation_entry, 
                            &mut target_sec_mapped_pages, 
                            target_sec.mapped_pages_offset, 
                            new_source_sec.start_address(), 
                            verbose_log
                        )?;

                        #[cfg(loscd_eval)] {
                            let end_rewriting_relocations = hpet.get_counter();
                            hpet_total_rewriting_relocations += end_rewriting_relocations - start_rewriting_relocations;
                        }

                        #[cfg(not(loscd_eval))] {
                            // If we temporarily remapped the target_sec's mapped pages as writable, undo that here
                            if !target_sec_initial_flags.is_writable() {
                                target_sec_mapped_pages.remap(&mut kernel_mmi_ref.lock().page_table, target_sec_initial_flags)?;
                            };
                        }
                    }
                

                    #[cfg(loscd_eval)]
                    let start_fixing_dependencies = hpet.get_counter();

                    // Tell the new source_sec that the existing target_sec depends on it.
                    // Note that we don't need to do this if we're re-swapping in a cached crate,
                    // because that crate's sections' dependents are already properly set up from when it was first swapped in.
                    if !is_optimized {
                        new_source_sec.inner.write().sections_dependent_on_me.push(WeakDependent {
                            section: Arc::downgrade(&target_sec),
                            relocation: relocation_entry,
                        });
                    }

                    // Tell the existing target_sec that it no longer depends on the old source section (old_sec),
                    // and that it now depends on the new source_sec.
                    let mut found_strong_dependency = false;
                    for mut strong_dep in target_sec.inner.write().sections_i_depend_on.iter_mut() {
                        if Arc::ptr_eq(&strong_dep.section, &old_sec) && strong_dep.relocation == relocation_entry {
                            strong_dep.section = Arc::clone(&new_source_sec);
                            found_strong_dependency = true;
                            break;
                        }
                    }
                    if !found_strong_dependency {
                        error!("Couldn't find/remove the existing StrongDependency from target_sec {:?} to old_sec {:?}",
                            target_sec.name, old_sec.name);
                        return Err("Couldn't find/remove the target_sec's StrongDependency on the old crate section");
                    }

                    #[cfg(loscd_eval)] {
                        let end_fixing_dependencies = hpet.get_counter();
                        hpet_total_fixing_dependencies += end_fixing_dependencies - start_fixing_dependencies;
                    }
                } // end of loop that iterates over all weak deps in the old_sec

                {
                    let mut old_sec_inner = old_sec.inner.write();
                    for index in dead_weak_deps_to_remove {
                        old_sec_inner.sections_dependent_on_me.remove(index);
                    }
                }
                
            } // end of loop that rewrites dependencies for sections that depend on the old_crate

            
        } // end of scope, drops lock on `new_crate_ref`
    } // end of iterating over all swap requests to fix up old crate dependents


    // Execute the provided state transfer functions
    for symbol in state_transfer_functions {
        let state_transfer_fn_sec = namespace_of_new_crates.get_symbol_or_load(&symbol, Some(this_namespace), kernel_mmi_ref, verbose_log).upgrade()
            // as a backup, search fuzzily to accommodate state transfer function symbol names without full hashes
            .or_else(|| namespace_of_new_crates.get_symbol_starting_with(&symbol).upgrade())
            .ok_or("couldn't find specified state transfer function in the new CrateNamespace")?;
        // FIXME SAFETY: None. swap_crates should probably be unsafe as there is no guaranteed that the state transfer functions have the correct signature.
        let st_fn = unsafe { state_transfer_fn_sec.as_func::<StateTransferFunction>() }?;
        #[cfg(not(loscd_eval))]
        debug!("swap_crates(): invoking the state transfer function {:?} with old_ns: {:?}, new_ns: {:?}", symbol, this_namespace.name(), namespace_of_new_crates.name());
        st_fn(this_namespace, &namespace_of_new_crates)?;
    }

    // Sanity check that we correctly populated the lists "new_crate_names" and "old_crates_are_loaded". 
    if swap_requests.len() != new_crate_names.len() &&  swap_requests.len() != old_crates_are_loaded.len() {
        return Err("BUG: swap_crates(): didn't properly populate the list of `new_crate_names` and/or `old_crates_are_loaded`.");
    }

    // Remove all of the old crates now that we're fully done using them.
    // This doesn't mean each crate will be immediately dropped -- they still might be in use by other crates or tasks.
    for ((req, new_crate_name), is_old_crate_loaded) in swap_requests.iter().zip(new_crate_names.iter()).zip(old_crates_are_loaded.iter()) {
        if !is_old_crate_loaded { continue; }
        let SwapRequest { old_crate_name, old_namespace, new_crate_object_file: _, new_namespace, reexport_new_symbols_as_old } = req;
        let old_crate_name = match old_crate_name {
            Some(ocn) => ocn,
            _ => continue,
        };
        // Remove the old crate from the namespace that it was previously in, and remove its sections' symbols too.
        if let Some(old_crate_ref) = old_namespace.crate_tree().lock().remove(old_crate_name.as_bytes()) {
            {
                let old_crate = old_crate_ref.lock_as_ref();

                core::mem::forget(old_crate_ref.clone());


                #[cfg(not(loscd_eval))]
                info!("  Removed old crate {:?} ({:?}) from namespace {}", old_crate_name, &*old_crate, old_namespace.name());

                if cache_old_crates {
                    #[cfg(not(loscd_eval))]
                    {
                        // Here, we setup the crate cache to enable the removed old crate to be quickly swapped back in in the future.
                        // This removed old crate will be useful when a future swap request includes the following:
                        // (1) the future `new_crate_object_file`        ==  the current `old_crate.object_file`
                        // (2) the future `old_crate_name`               ==  the current `new_crate_name`
                        // (3) the future `reexport_new_symbols_as_old`  ==  true if the old crate had any reexported symbols
                        //     -- to understand this, see the docs for `LoadedCrate.reexported_prefix`
                        let future_swap_req = SwapRequest {
                            old_crate_name: Some(new_crate_name.clone()),
                            old_namespace: ByAddress(Arc::clone(new_namespace)),
                            new_crate_object_file: ByAddress(old_crate.object_file.clone()),
                            new_namespace: ByAddress(Arc::clone(old_namespace)),
                            reexport_new_symbols_as_old: !old_crate.reexported_symbols.is_empty(),
                        };
                        future_swap_requests.push(future_swap_req);
                    }
                }
                
                // Remove all of the symbols belonging to the old crate from the namespace it was in.
                // If reexport_new_symbols_as_old is true, we MUST NOT remove the old_crate's symbols from this symbol map,
                // because we already replaced them above with mappings that redirect to the corresponding new crate sections.
                if !reexport_new_symbols_as_old {
                    let mut old_ns_symbol_map = old_namespace.symbol_map().lock();
                    for old_sec in old_crate.global_sections_iter() {
                        if old_ns_symbol_map.remove(&old_sec.name).is_none() {
                            error!("swap_crates(): couldn't find old symbol {:?} in the old crate's namespace: {}.", old_sec.name, old_namespace.name());
                            return Err("couldn't find old symbol {:?} in the old crate's namespace");
                        }
                    }
                }

                // If the old crate had reexported its symbols, we should remove those reexports here,
                // because they're no longer active since the old crate is being removed. 
                for sym in &old_crate.reexported_symbols {
                    let _old_reexported_symbol = old_namespace.symbol_map().lock().remove(sym);
                    if _old_reexported_symbol.is_none() {
                        warn!("swap_crates(): the old_crate {:?}'s reexported symbol was not in its old namespace, couldn't be removed.", sym);
                    }
                }

                if cache_old_crates {
                    // TODO: could maybe optimize transfer of old symbols from this namespace to cached_crates namespace 
                    //       by saving the removed symbols above and directly adding them to the cached_crates.symbol_map instead of iterating over all old_crate.sections.
                    //       This wil only really be faster once qp_trie supports a non-iterator-based (non-extend) Trie merging function.
                    #[cfg(not(loscd_eval))]
                    cached_crates.add_symbols(old_crate.sections.values(), verbose_log); 
                }
            } // drops lock for `old_crate_ref`
            
            if cache_old_crates {
                #[cfg(not(loscd_eval))]
                cached_crates.crate_tree().lock().insert(old_crate_name.as_str().into(), old_crate_ref);
            }

            #[cfg(loscd_eval)]
            core::mem::forget(old_crate_ref);
        }
        else {
            error!("swap_crates(): couldn't remove old crate {} from old namespace {}!", old_crate_name, old_namespace.name());
            continue;
        }
    }

    #[cfg(loscd_eval)]
    let start_symbol_cleanup = hpet.get_counter();

    // Here, we move all of the new crates into the actual new namespace where they belong. 
    for ((req, new_crate_name), is_old_crate_loaded) in swap_requests.iter().zip(new_crate_names.iter()).zip(old_crates_are_loaded.iter()) {
        // We only expect the new crate to have been loaded into the temp namespace if the old crate was actually loaded in the old namespace
        if !is_old_crate_loaded { continue; }
        let new_crate_ref = namespace_of_new_crates.crate_tree().lock().remove(new_crate_name.as_bytes())
            .ok_or("BUG: swap_crates(): new crate specified by swap request was not found in the new namespace")?;
        
        #[cfg(not(loscd_eval))]
        debug!("swap_crates(): adding new crate {:?} to namespace {}", new_crate_ref, req.new_namespace.name());

        req.new_namespace.add_symbols(new_crate_ref.lock_as_ref().sections.values(), verbose_log);
        req.new_namespace.crate_tree().lock().insert(new_crate_name.as_str().into(), new_crate_ref.clone());
    }
    
    // Other crates may have been loaded from their object files into the `namespace_of_new_crates` as dependendencies (required by the new crates specified by swap requests).
    // Thus, we need to move all **newly-loaded** crates from the `namespace_of_new_crates` into the proper new namespace;
    // for this, we add only the non-shared (exclusive) crates, because shared crates are those that were previously loaded (and came from the backup namespace).
    namespace_of_new_crates.for_each_crate(true, |new_crate_name, new_crate_ref| {
        if !new_crate_ref.is_shared() {
            // TODO FIXME: we may not want to add the new crate to this namespace, we might want to add it to one of its recursive namespaces
            //             (e.g., `this_namespace` could be an application ns, but the new_crate could be a kernel crate that belongs in `this_namespaces`'s recursive kernel namespace).
            //
            // To infer which actual namespace this newly-loaded crate should be added to (we don't want to add kernel crates to an application namespace),
            // we could iterate over all of the new crates in the swap requests that transitively depend on this new_crate,
            // and then look at which `new_namespace` those new crates in the swap requests were destined for.
            // Then, we use the highest-level namespace that we can, but it cannot be higher than any one `new_namespace'.
            // 
            // As an example, suppose there are 3 new crates from the swap request list that depend on this new_crate_ref here. 
            // If two of those were application crates that belong to a higher-level "applications" namespace (their `new_namespace`), 
            // but one of them was a kernel crate that belonged to a lower "kernel" namespace that was found in that "applications" namespace recursive children, 
            // the highest-level namespace we could add this `new_crate_ref` to would be that "kernel" namespace,
            // meaning that we had inferred that this `new_crate_ref` was also a kernel crate that belonged in the "kernel" namespace.
            //
            // This follows the rule that crates in a lower-level namespace cannot depend on those in a higher-level namespace. 
            //
            // Note that we don't just want to put the crate in the lowest namespace we can, 
            // because that could result in putting an application crate in a kernel namespace. 
            // 
            let mut target_ns = this_namespace;

            // FIXME: currently we use a hack to determine which namespace this freshly-loaded crate should be added to,
            //        based on which directory its object file 
            {
                let objfile_path = Path::new(new_crate_ref.lock_as_ref().object_file.lock().get_absolute_path());
                if objfile_path.components().skip(1).next() == Some(mod_mgmt::CrateType::Kernel.default_namespace_name()) {
                    let new_target_ns = this_namespace.recursive_namespace().unwrap_or(this_namespace);
                    #[cfg(not(loscd_eval))]
                    warn!("temp fix: changing target_ns from {} to {}, for crate {:?}", this_namespace.name(), new_target_ns.name(), new_crate_ref);
                    target_ns = new_target_ns;
                }

            }

            // #[cfg(not(loscd_eval))]
            // warn!("swap_crates(): untested scenario of adding new non-requested (dependency) crate {:?} to namespace {}", new_crate_ref, target_ns.name());
            target_ns.add_symbols(new_crate_ref.lock_as_ref().sections.values(), verbose_log);
            target_ns.crate_tree().lock().insert(new_crate_name.into(), new_crate_ref.clone());
        }
        else {
            #[cfg(not(loscd_eval))] {
                if this_namespace.get_crate(new_crate_name).is_none() { 
                    error!("BUG: shared crate {} was not already in the current (backup) namespace", new_crate_name);
                }
                // else {
                //     debug!("shared crate {} was in the current namespace like we expected.", new_crate_name);
                // }
            }
        }
        true
    });

    #[cfg(loscd_eval)]
    let end_symbol_cleanup = hpet.get_counter();

    // Here, we move all of the new crate object files from the temp namespace directory into the namespace directory where they belong,
    // and any crate object files that get replaced will be moved to the temp namespace directory. 
    // This ensures that future usage of the newly swapped-in crates will use the new updated crate object files, not the old ones. 
    // Effectively, we're swapping the new crate object file with the old. 
    // Also, since the SwapRequest struct uses direct file references, we don't need to update them when we move the file. 
    for req in swap_requests.iter() {
        let SwapRequest { old_crate_name, old_namespace, new_crate_object_file, new_namespace, reexport_new_symbols_as_old: _ } = req;

        let source_dir_ref = new_crate_object_file.lock().get_parent_dir().ok_or("BUG: new_crate_object_file has no parent directory")?;
        let dest_dir_ref   = new_namespace.dir().deref();
        // // If the directories are the same (not overridden), we don't need to do anything.
        if Arc::ptr_eq(&source_dir_ref, dest_dir_ref) {
            #[cfg(not(any(loscd_eval, downtime_eval)))]
            trace!("swap_crates(): skipping crate file swap for {:?}", req);
            continue;
        }

        // Move the new crate object file from the temp namespace dir into the namespace dir that it belongs to.
        if let Some((mut replaced_old_crate_file, original_source_dir)) = move_file(new_crate_object_file, dest_dir_ref)? {
            // If we replaced a crate object file, put that replaced file back in the source directory, thus completing the "swap" operation.
            // (Note that the file that we replaced should be the same as the old_crate_file.) 
            #[cfg(not(any(loscd_eval, downtime_eval)))]
            trace!("swap_crates(): new_crate_object_file replaced existing (old_crate) object file {:?}", replaced_old_crate_file.get_name());

            replaced_old_crate_file.set_parent_dir(Arc::downgrade(&original_source_dir));
            if let Some(_f) = original_source_dir.lock().insert(replaced_old_crate_file)? {
                // There shouldn't be a similarly-named file in the original source dir anymore, since we moved it.
                // However, this isn't necessarily a real problem; we can continue execution, but I'd like to log an error for sanity checking purposes.
                error!("swap_crates(): unexpectedly replaced file {:?} that was in source directory {:?}", _f.get_name(), original_source_dir.lock().get_absolute_path());
            }
        } else {
            // If inserting the new crate object file didn't end up replacing the existing crate object file (the old_crate's object file), 
            // then we need to remove the old_crate's object file here, if one was specified. 
            if let Some(ocn) = old_crate_name {
                #[cfg(not(any(loscd_eval, downtime_eval)))]
                trace!("swap_crates(): new_crate_object_file did not replace old_crate's object file, so we're removing the old_crate's object file now");
                let (old_crate_object_file, _old_ns) = CrateNamespace::get_crate_object_file_starting_with(old_namespace, &*ocn).ok_or_else(|| {
                    error!("BUG: swap_crates(): couldn't find old crate's object file starting with {:?} in old namespace {:?}.", ocn, old_namespace.name());
                    "BUG: swap_crates(): couldn't find old crate's object file in old namespace!"
                })?;
                let mut removed_old_crate_file = old_namespace.dir().lock().remove(&FileOrDir::File(Arc::clone(&old_crate_object_file))).ok_or_else(|| {
                    error!("BUG: swap_crates(): couldn't remove old crate's object file {:?} from old namespace {:?}.", old_crate_object_file.lock().get_name(), old_namespace.name());
                    "BUG: swap_crates(): couldn't remove old crate's object file from old namespace!"
                })?;
                removed_old_crate_file.set_parent_dir(Arc::downgrade(&source_dir_ref));
                if let Some(_f) = source_dir_ref.lock().insert(removed_old_crate_file)? {
                    // This is not necessarily a problem, but is currently unexpected behavior.
                    warn!("swap_crates(): unexpectedly replaced file {:?} that was in source directory {:?}", _f.get_name(), source_dir_ref.lock().get_absolute_path());
                } 
            } else {
                // If there's no old crate to be replaced (we're just adding a new crate), then we don't need to do anything here. 
            }
        }
    }

    if cache_old_crates {
        #[cfg(not(loscd_eval))]
        {
            debug!("swap_crates() [end]: adding old_crates to cache. \n   future_swap_requests: {:?}, \n   old_crates: {:?}", 
                future_swap_requests, cached_crates.crate_names(true));
            UNLOADED_CRATE_CACHE.lock().insert(future_swap_requests, cached_crates);
        }
    }


    #[cfg(all(loscd_eval, not(downtime_eval)))] {
        // done with everything, print out values

        warn!("Measured time in units of HPET ticks:
            load crates, {}
            find symbols, {}
            rewrite relocations, {}
            fix dependencies, {}
            BSS transfer, {}
            symbol cleanup, {}
            HPET PERIOD (femtosec): {}
            ",
            hpet_after_load_crates - hpet_start_swap,
            hpet_total_symbol_finding,
            hpet_total_rewriting_relocations,
            hpet_total_fixing_dependencies,
            hpet_total_bss_transfer,
            end_symbol_cleanup - start_symbol_cleanup,
            hpet.counter_period_femtoseconds(),
        );
    }

    Ok(())
    // here, "namespace_of_new_crates is dropped, but its crates have already been added to the current namespace 
}


/// Convenience function that removes the given `file` from its parent directory 
/// and inserts it into the given destination directory. 
/// 
/// # Return
/// If the file ends up replacing a file/dir node in the `dest_dir`, this returns a tuple of:
/// 1. The node that was in the `dest_dir` that got replaced by the given `file`,
/// 2. The parent directory that originally contained the given `file`. This is useful for realizing a file "swap" operation.
/// 
/// # Locking / Deadlock
/// This function obtains the lock on both `file`, the `file`'s parent directory, and the `dest_dir`. 
fn move_file(file: &FileRef, dest_dir: &DirRef) -> Result<Option<(FileOrDir, DirRef)>, &'static str> {
    let parent = file.lock().get_parent_dir().ok_or("couldn't get file's parent directory")?;
    // This section is redundent as it is checked before calling the function
    // if Arc::ptr_eq(&parent, dest_dir) {
    //     #[cfg(not(downtime_eval))]
    //     trace!("swap_crates::move_file(): skipping move between same directory {:?} for file {:?}", 
    //         dest_dir.try_lock().map(|f| f.get_absolute_path()), file.try_lock().map(|f| f.get_absolute_path())
    //     );
    //     return Ok(None);
    // }

    // Perform the actual move operation.
    let mut removed_file = parent.lock().remove(&FileOrDir::File(Arc::clone(file))).ok_or("Couldn't remove file from its parent directory")?;
    removed_file.set_parent_dir(Arc::downgrade(dest_dir));
    let res = dest_dir.lock().insert(removed_file.clone());
    
    // Log success or failure
    match res {
        Ok(replaced_file) => {
            #[cfg(not(any(loscd_eval, downtime_eval)))]
            debug!("swap_crates::move_file(): moved file {:?} ({:?}) from {:?} to {:?}",
                file.try_lock().map(|f| f.get_name()), removed_file.get_name(), parent.try_lock().map(|p| p.get_name()), dest_dir.try_lock().map(|d| d.get_name())
            );
            Ok(replaced_file.map(|f| (f, parent)))
        }
        Err(e) => {
            #[cfg(not(any(loscd_eval, downtime_eval)))]
            error!("swap_crates::move_file(): failed to moved file {:?} ({:?}) from {:?} to {:?}.\n    Error: {:?}",
                file.try_lock().map(|f| f.get_name()), removed_file.get_name(), parent.try_lock().map(|p| p.get_name()), dest_dir.try_lock().map(|d| d.get_name()), e
            );
            Err(e)
        }
    }
}


/// A list of one or more `SwapRequest`s that is used by the `swap_crates` function.
pub type SwapRequestList = Vec<SwapRequest>;

/// This struct is used to specify the details of a crate-swapping operation,
/// in which an "old" crate is replaced with a "new" crate that is then used in place of that old crate. 
/// The old crate is removed from its old `CrateNamespace` while the new crate 
/// is added to the new `CrateNamespace`; typically the old and new namespaces are the same. 
/// 
/// # Important Note: Reexporting Symbols
/// When swapping out an old crate, the new crate must satisfy all of the dependencies
/// that other crates had on that old crate. 
/// However, the swapping procedure will completely remove the old crate and its symbols from its containing `CrateNamespace`,
/// so it may be useful to re-expose or "mirror" the new crate's sections as symbols with names 
/// that match the relevant sections from the old crate.
/// This satisfies other crates' dependenices on the old crate while allowing the new crate to exist normally,
/// which means that the new crate's symbols appear both as those from the new crate itself 
/// in addition to those from the old crate that was replaced. 
/// To do this, set the `reexport_new_symbols_as_old` option to `true`.
/// 
/// However, **enabling this option is potentially dangerous**, as you must take responsibility 
/// for ensuring that the new crate can safely and correctly replace the old crate, 
/// and that using the new crate in place of the old crate will not break any other dependent crates.
/// This will necessarily ignore the hashes when matching symbols across the old and new crate, 
/// and hashes are how Theseus ensures that different crates and functions are linked to the correct version of each other.
/// However, this does make the swapping operation much simpler to use, 
/// because you will likely need to swap far fewer crates at once, 
/// and as a convenience bonus, you don't have to specify the exact versions (hashes) of each crate.
/// 
/// ## Example of Reexporting Symbols
/// There exists a function `drivers::init::h...` in the `drivers` crate 
/// that depends on the function `keyboard::init::hABC` in the `keyboard` crate.
/// We call `swap_crates()` to replace the `keyboard` crate with a new crate called `keyboard_new`.
/// 
/// If `reexport_new_symbols_as_old` is `true`, the new `keyboard_new` crate's function called `keyboard_new::init::hDEF`
/// will now appear in the namespace's symbol map twice: 
/// 1. in its normal form `keyboard_new::init::hDEF`, which is the normal behavior when loading a crate, and
/// 2. in its reexported form `keyboard::init::hABC`, which exactly matches the function from the old `keyboard` crate.
/// 
/// In this way, the single function in the new crate `keyboard_new` appears twice in the symbol map
/// under different names, allowing it to fulfill dependencies on both the old crate and the new crate.
/// In general, this option is not needed. 
/// 
#[derive(Eq, PartialEq, Hash)]
pub struct SwapRequest {
    // Note: the usage of `ByAddress` is to allow us to hash and compare reference types like Arc
    //       to make sure that they point to the same file rather than having the same actual contents.
    //       It makes hashing extremely cheap and fast!

    /// The full name of the old crate that will be replaced by the new crate.
    /// This will be used to search the `CrateNamespace` to find an existing `LoadedCrate`.
    /// The `SwapRequest` constructor ensures this is fully-qualified (includes a hash value)
    /// and is unique within the `old_namespace`.
    old_crate_name: Option<String>,
    /// The `CrateNamespace` that contains the given old crate, 
    /// from which that old crate and its symbols should be removed. 
    old_namespace: ByAddress<Arc<CrateNamespace>>,
    /// The object file for the new crate that will replace the old crate. 
    new_crate_object_file: ByAddress<FileRef>,
    /// The `CrateNamespace` into which the replacement new crate and its symbols should be loaded.
    /// Typically, this is the same namespace as the `old_namespace`.
    new_namespace: ByAddress<Arc<CrateNamespace>>,
    /// Whether to expose the new crate's sections with symbol names that match those from the old crate.
    /// For more details, see the above docs for this struct.
    reexport_new_symbols_as_old: bool,
}
impl fmt::Debug for SwapRequest {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("SwapRequest")
            .field("old_crate", &self.old_crate_name)
            .field("old_namespace", &self.old_namespace.name())
            .field("new_crate", &self.new_crate_object_file.try_lock()
                .map(|f| f.get_absolute_path())
                .unwrap_or_else(|| format!("<Locked>"))
            )
            .field("new_namespace", &self.new_namespace.name())
            .field("reexport_symbols", &self.reexport_new_symbols_as_old)
            .finish()
    }
}
impl SwapRequest {
    /// Create a new `SwapRequest` that, when given to `swap_crates()`, 
    /// will swap out the given old crate and replace it with the given new crate,
    /// and optionally re-export the new crate's symbols.
    /// 
    /// # Arguments
    /// * `old_crate_name`: the name of the old crate that should be unloaded and removed from the `old_namespace`. 
    ///    An `old_crate_name` of `None` (or an empty string) signifies there is no old crate to be removed,
    ///    and that only a new crate will be added.
    /// 
    ///    Note that `old_crate_name` can be any string prefix, so long as it can uniquely identify a crate object file
    ///    in the given `old_namespace` or any of its recursive namespaces. 
    ///    Thus, to be more accurate, it is wise to specify a full crate name with hash, e.g., "my_crate-<hash>".
    /// 
    /// * `old_namespace`: the `CrateNamespace` that contains the old crate;
    ///    that old crate and its symbols will be removed from this namespace. 
    /// 
    ///    If `old_crate_name` is `None`, then `old_namespace` should be the same namespace 
    ///    as the `this_namespace` argument that `swap_crates()` is invoked with. 
    /// 
    /// * `new_crate_object_file`: a type that can be converted into a crate object file.
    ///    This can either be a direct reference to the file, an absolute `Path` that points to the file,
    ///    or a prefix string used to find the file in the new namespace's directory of crate object files. 
    /// 
    /// * `new_namespace`: the `CrateNamespace` to which the new crate will be loaded and its symbols added.
    ///    If `None`, then the new crate will be loaded into the `old_namespace`, which is a common case for swapping. 
    /// 
    /// * `reexport_new_symbols_as_old`: if `true`, all public symbols the new crate will be reexported
    ///    in the `new_namespace` with the same full name those symbols had from the old crate in the `old_namespace`. 
    ///    See the "Important Note" in the struct-level documentation for more details.
    /// 
    pub fn new(
        // We could change this to `Option<&str>`
        old_crate_name: Option<&str>,
        old_namespace: Arc<CrateNamespace>,
        new_crate_object_file: IntoCrateObjectFile,
        new_namespace: Option<Arc<CrateNamespace>>,
        reexport_new_symbols_as_old: bool,
    ) -> Result<SwapRequest, InvalidSwapRequest> {
        // Check that the old crate is actually in the old namespace; 
        // it may be currently loaded into the old namespace, 
        // but if not, we look to see if its crate object file is there.
        let (old_crate_full_name, real_old_namespace) = match old_crate_name {
            None | Some("") => {
                // If the old crate name is empty, that means there is no old crate to replace. 
                (None, &old_namespace)
            }    
            Some(ocn) => {
                // Look for a single loaded crate that matches the `old_crate_name` prefix.
                let mut matching_crates = CrateNamespace::get_crates_starting_with(&old_namespace, ocn);
                if matching_crates.len() == 1 {
                    let (old_crate_full_name, _ocr, real_old_namespace) = matching_crates.remove(0);
                    (Some(old_crate_full_name.to_string()), real_old_namespace)
                } else {
                    // If we couldn't find a single loaded crate, then the old crate may not be loaded yet. 
                    // Thus, we should instead look for a single crate **object file** that matches the `old_crate_name` prefix.
                    let mut matching_files = CrateNamespace::get_crate_object_files_starting_with(&old_namespace, ocn);
                    if matching_files.len() == 1 {
                        let (old_crate_file, real_old_namespace) = matching_files.remove(0);
                        let old_crate_file_path = Path::new(old_crate_file.lock().get_name());
                        let old_crate_full_name = crate_name_from_path(&old_crate_file_path).to_string();
                        (Some(old_crate_full_name), real_old_namespace)
                    } else {
                        // Here, we couldn't find a single matching loaded crate or crate object file, so we return an error. 
                        let matches_vec = if !matching_crates.is_empty() {
                            matching_crates.into_iter().map(|(c_name, _c_ref, ns)| (c_name.to_string(), Arc::clone(ns))).collect::<Vec<_>>()
                        } else if !matching_files.is_empty() {
                            matching_files.into_iter()
                                .map(|(file, ns)| (
                                    file.try_lock().map(|f| f.get_absolute_path()).unwrap_or_else(|| format!("<Locked>")),
                                    Arc::clone(ns)
                                ))
                                .collect::<Vec<_>>()
                        } else {
                            Vec::new()
                        };
                        return Err(InvalidSwapRequest::OldCrateNotFound(
                            old_crate_name.map(ToString::to_string),
                            old_namespace,
                            matches_vec,
                        ));
                    }

                }

            }
        };

        if !Arc::ptr_eq(&old_namespace, real_old_namespace) {
            #[cfg(not(downtime_eval))]
            trace!("SwapRequest::new(): changing old namespace from {:?} to {:?}", old_namespace.name(), real_old_namespace.name());
        }
        
        // If no new namespace was given, use the same namespace that the old crate was found in.
        let mut new_namespace = new_namespace.unwrap_or_else(|| {
            #[cfg(not(downtime_eval))]
            trace!("SwapRequest::new(): new namespace was None, using old namespace {:?}", real_old_namespace.name());
            Arc::clone(real_old_namespace)
        });

        // Try to resolve the new crate argument into an actual file.
        let verified_new_crate_file = match new_crate_object_file {
            IntoCrateObjectFile::File(f) => f,
            IntoCrateObjectFile::AbsolutePath(path) => match Path::get_absolute(&path) {
                Some(FileOrDir::File(f)) => f,
                _ => if path.is_absolute() {
                    return Err(InvalidSwapRequest::NewCrateAbsolutePathNotFound(path));
                } else {
                    return Err(InvalidSwapRequest::NewCratePathNotAbsolute(path));
                },
            }
            IntoCrateObjectFile::Prefix(prefix) => {
                let (new_crate_file, real_new_namespace) = {
                    let mut matching_files = CrateNamespace::get_crate_object_files_starting_with(&new_namespace, &prefix);
                    if matching_files.len() == 1 {
                        matching_files.remove(0)
                    } else {
                        return Err(InvalidSwapRequest::NewCratePrefixNotFound(
                            prefix, 
                            Arc::clone(&new_namespace),
                            matching_files.into_iter().map(|(f, ns_ref)| (f, Arc::clone(ns_ref))).collect::<Vec<_>>()
                        ));
                    }
                };
                if !Arc::ptr_eq(&new_namespace, real_new_namespace) {
                    #[cfg(not(downtime_eval))]
                    trace!("SwapRequest::new(): changing new namespace from {:?} to {:?}", new_namespace.name(), real_new_namespace.name());
                    new_namespace = Arc::clone(real_new_namespace);
                }
                new_crate_file
            }
        };

        Ok(SwapRequest {
            old_crate_name: old_crate_full_name,
            old_namespace: ByAddress(Arc::clone(real_old_namespace)),
            new_crate_object_file: ByAddress(verified_new_crate_file),
            new_namespace: ByAddress(new_namespace),
            reexport_new_symbols_as_old,
        })
    }
}

/// The possible errors that can occur when trying to create a valid `SwapRequest`. 
pub enum InvalidSwapRequest {
    /// The old crate was not found in the old `CrateNamespace`.
    /// The enclosed `String` is the `old_crate_name` passed into `SwapRequest::new()`.
    /// The enclosed vector is the list of matching crate names or crate object file names 
    /// along with the `CrateNamespace` in which they were found. 
    OldCrateNotFound(Option<String>, Arc<CrateNamespace>, Vec<(String, Arc<CrateNamespace>)>),
    /// The given absolute `Path` for the new crate object file could not be resolved.
    NewCrateAbsolutePathNotFound(Path),
    /// The given `Path` for the new crate object file was not an absolute path, as expected.
    NewCratePathNotAbsolute(Path),
    /// A single crate object file could not be found by matching the given prefix `String`
    /// within the given new `CrateNamespace` (which was searched recursively).
    /// Either zero or multiple crate object files matched the prefix,
    /// the results of the match are given by the enclosed vector. 
    NewCratePrefixNotFound(String, Arc<CrateNamespace>, Vec<(FileRef, Arc<CrateNamespace>)>),
}
impl fmt::Debug for InvalidSwapRequest {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut dbg = f.debug_struct("InvalidSwapRequest");
        match self {
            Self::OldCrateNotFound(old_crate_name, old_namespace, matches) => {
                if matches.is_empty() {
                    dbg.field("reason", &"No Matches for Old Crate Name");
                } else {
                    dbg.field("reason", &"Multiple Matches for Old Crate Name");
                }
                dbg.field("old_crate_name", old_crate_name)
                    .field("old_namespace", &old_namespace.name());
                for (f, ns) in matches {
                    dbg.field("match", &format!("{:?} in namespace {:?}", f, ns.name()));
                }
            }
            Self::NewCrateAbsolutePathNotFound(path) => {
                dbg.field("reason", &"New Crate Absolute Path Not Found")
                    .field("path", &path);
            }
            Self::NewCratePathNotAbsolute(path) => {
                dbg.field("reason", &"New Crate Path Not Absolute")
                    .field("path", &path);
            }
            Self::NewCratePrefixNotFound(prefix, new_namespace, matches) => {
                if matches.is_empty() {
                    dbg.field("reason", &"No Matches for New Crate File Prefix");
                } else {
                    dbg.field("reason", &"Multiple Matches for New Crate File Prefix");
                }
                dbg.field("prefix", &prefix)
                    .field("searched in new_namespace", &new_namespace.name());
                for (file, ns) in matches {
                    let s = format!("{:?} in namespace {:?}",
                        file.try_lock().map(|f| f.get_absolute_path()).unwrap_or_else(|| format!("<Locked>")),
                        ns.name(),
                    );
                    dbg.field("matching file", &s);
                }
            }
        };
        dbg.finish()
    }
}
