//! Routines for converting serializable crate metadata types
//! into the actual runtime `LoadedCrate` and `LoadedSection` types.
//! 
//! See [`crate_metadata_serde`] docs for more information.

use crate::*;
use crate_metadata_serde::{SerializedCrate, SerializedSection};

/// Converts the given [`SerializedCrate`] into a [`LoadedCrate`]
/// and its sections into [`LoadedSection`]s.
pub(crate) fn into_loaded_crate(
    serialized_crate: SerializedCrate,
    object_file: FileRef,
    namespace: &Arc<CrateNamespace>,
    text_pages: &Arc<Mutex<MappedPages>>,
    rodata_pages: &Arc<Mutex<MappedPages>>,
    data_pages: &Arc<Mutex<MappedPages>>,
    verbose_log: bool,
) -> Result<(StrongCrateRef, BTreeMap<String, usize>, usize), &'static str> {
    let crate_name: StrRef = serialized_crate.crate_name.as_str().into();

    let total_tls_size: usize = serialized_crate.tls_sections
        .iter()
        .filter_map(|shndx| serialized_crate.sections.get(shndx))
        .map(|tls_sec| tls_sec.size)
        .sum();

    let total_cls_size: usize = serialized_crate.cls_sections
        .iter()
        .filter_map(|shndx| serialized_crate.sections.get(shndx))
        .map(|cls_sec| cls_sec.size)
        .sum();

    
    // The sections need a weak reference back to the loaded_crate, and so we first create
    // the loaded_crate so we have something to reference when loading the sections.
    let loaded_crate = CowArc::new(LoadedCrate {
        crate_name:          crate_name.clone(),
        debug_symbols_file:  Arc::downgrade(&object_file),
        object_file,
        sections:            HashMap::new(), // placeholder
        text_pages:          Some((Arc::clone(text_pages), mp_range(text_pages))),
        rodata_pages:        Some((Arc::clone(rodata_pages), mp_range(rodata_pages))),
        data_pages:          Some((Arc::clone(data_pages), mp_range(data_pages))),
        global_sections:     serialized_crate.global_sections,
        tls_sections:        serialized_crate.tls_sections,
        cls_sections:        serialized_crate.cls_sections,
        data_sections:       serialized_crate.data_sections,
        reexported_symbols:  BTreeSet::new(),
    });
    let parent_crate_weak_ref = CowArc::downgrade(&loaded_crate);

    let mut sections = HashMap::with_capacity(serialized_crate.sections.len());
    for (shndx, section) in serialized_crate.sections {
        // Skip zero-sized TLS sections, which are just markers, not real sections.
        if section.ty.is_tls() && section.size == 0 {
            continue;
        }
        sections.insert(
            shndx,
            into_loaded_section(
                section,
                parent_crate_weak_ref.clone(),
                namespace,
                text_pages,
                rodata_pages,
                data_pages,
                total_tls_size,
                total_cls_size,
            )?,
        );
    }

    let num_new_syms = namespace.add_symbols(sections.values(), verbose_log);

    let mut loaded_crate_mut = loaded_crate.lock_as_mut().ok_or(
        "BUG: SerializedCrate::into_loaded_crate(): couldn't get exclusive mutable access to loaded_crate",
    )?;
    loaded_crate_mut.sections = sections;
    drop(loaded_crate_mut);

    // Add the newly-parsed nano_core crate to the kernel namespace.
    namespace.crate_tree.lock().insert(crate_name, loaded_crate.clone_shallow());
    info!("Finished parsing nano_core crate, added {} new symbols.", num_new_syms);
    
    // // Dump loaded sections for verification. See pull request #542/#559 for more details:
    // let loaded_crate_ref = loaded_crate.lock_as_ref();
    // for (_, section) in loaded_crate_ref.sections.iter() {
    //     trace!("{:016x} {} {}", section.virt_addr.value(), section.name, section.mapped_pages_offset);
    // }
    // drop(loaded_crate_ref);

    Ok((loaded_crate, serialized_crate.init_symbols, num_new_syms))
}


/// Convert the given [`SerializedSection`] into a [`LoadedSection`].
#[allow(clippy::too_many_arguments)]
fn into_loaded_section(
    serialized_section: SerializedSection,
    parent_crate:       WeakCrateRef,
    namespace:          &Arc<CrateNamespace>,
    text_pages:         &Arc<Mutex<MappedPages>>,
    rodata_pages:       &Arc<Mutex<MappedPages>>,
    data_pages:         &Arc<Mutex<MappedPages>>,
    total_tls_size:     usize,
    total_cls_size:     usize,
) -> Result<Arc<LoadedSection>, &'static str> {
    let mapped_pages = match serialized_section.ty {
        SectionType::Text => Arc::clone(text_pages),
        SectionType::Rodata
        | SectionType::TlsData
        | SectionType::TlsBss
        | SectionType::Cls
        | SectionType::GccExceptTable
        | SectionType::EhFrame => Arc::clone(rodata_pages),
        SectionType::Data
        | SectionType::Bss => Arc::clone(data_pages),
    };
    let virt_addr = VirtualAddress::new(serialized_section.virtual_address)
        .ok_or("SerializedSection::into_loaded_section(): invalid virtual address")?;

    let loaded_section = LoadedSection::new(
        serialized_section.ty,
        match serialized_section.ty {
            SectionType::EhFrame
            | SectionType::GccExceptTable => crate::section_name_str_ref(&serialized_section.ty),
            _ => serialized_section.name.as_str().into(),
        },
        mapped_pages,
        serialized_section.offset,
        virt_addr,
        serialized_section.size,
        serialized_section.global,
        parent_crate,
    );

    if serialized_section.ty.is_tls() {
        namespace.tls_initializer.lock().add_existing_static_section(
            loaded_section,
            // TLS sections encode their TLS offset in the virtual address field,
            // which is necessary to properly calculate relocation entries that depend upon them.
            serialized_section.virtual_address,
            total_tls_size,
        ).map_err(|_| "BUG: failed to add deserialized static TLS section to the TLS area")
        // On AArch64, the linker includes a _TLS_MODULE_BASE_ zero sized symbol that we don't want to add.
    } else if serialized_section.ty == SectionType::Cls && serialized_section.size > 0 {
        cls_allocator::add_static_section(loaded_section, serialized_section.virtual_address, total_cls_size).map_err(|e| panic!("{:?}", e))
    } else {
        Ok(Arc::new(loaded_section))
    }
}
