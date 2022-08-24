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
        data_sections:       serialized_crate.data_sections,
        reexported_symbols:  BTreeSet::new(),
    });

    let mut sections = HashMap::with_capacity(serialized_crate.sections.len());
    for (shndx, section) in serialized_crate.sections {
        sections.insert(
            shndx,
            into_loaded_section(
                section,
                CowArc::downgrade(&loaded_crate),
                namespace,
                text_pages,
                rodata_pages,
                data_pages,
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
    //     trace!("{:016x} {} {}", section.address_range.start.value(), section.name, section.mapped_pages_offset);
    // }
    // drop(loaded_crate_ref);

    Ok((loaded_crate, serialized_crate.init_symbols, num_new_syms))
}


/// Convert the given [`SerializedSection`] into a [`LoadedSection`].
fn into_loaded_section(
    serialized_section: SerializedSection,
    parent_crate: WeakCrateRef,
    namespace: &Arc<CrateNamespace>,
    text_pages: &Arc<Mutex<MappedPages>>,
    rodata_pages: &Arc<Mutex<MappedPages>>,
    data_pages: &Arc<Mutex<MappedPages>>,
) -> Result<Arc<LoadedSection>, &'static str> {
    let mapped_pages = match serialized_section.ty {
        SectionType::Text => Arc::clone(text_pages),
        SectionType::Rodata
        | SectionType::TlsData
        | SectionType::TlsBss
        | SectionType::GccExceptTable
        | SectionType::EhFrame => Arc::clone(rodata_pages),
        SectionType::Data | SectionType::Bss => Arc::clone(data_pages),
    };
    let virtual_address = VirtualAddress::new(serialized_section.virtual_address)
        .ok_or("SerializedSection::into_loaded_section(): invalid virtual address")?;

    let loaded_section = Arc::new(LoadedSection {
        name: match serialized_section.ty {
            SectionType::EhFrame
            | SectionType::GccExceptTable => crate::section_name_str_ref(&serialized_section.ty),
            _ => serialized_section.name.as_str().into(),
        },
        typ: serialized_section.ty,
        global: serialized_section.global,
        mapped_pages_offset: serialized_section.offset,
        mapped_pages,
        address_range: virtual_address..(virtual_address + serialized_section.size),
        parent_crate,
        inner: Default::default(),
    });

    if let SectionType::TlsData | SectionType::TlsBss = serialized_section.ty {
        namespace.tls_initializer.lock().add_existing_static_tls_section(
            // TLS sections encode their TLS offset in the virtual address field,
            // which is necessary to properly calculate relocation entries that depend upon them.
            serialized_section.virtual_address,
            Arc::clone(&loaded_section),
        )
        .unwrap();
    }

    Ok(loaded_section)
}
