//! Serializable versions of crate and section metadata types.
//!
//! The primary reason this exists is because [`LoadedCrate`] and [`LoadedSection`]
//! make copious usage of `Arc` and `Weak` reference-counted pointer types,
//! which cannot be properly (de)serialized by `serde`. 
//! The types in this module remove those refcount types and then reconstruct them
//! individually at runtime after deserialization.
//!
//! This is currently only used to parse and serialize the `nano_core` binary at compile time.
//! The `nano_core`'s [`SerializedCrate`] is then included as a boot module
//! so it can be deserialized into a [`LoadedCrate`] at runtime.

use crate::{CrateNamespace, mp_range};
use alloc::{
    collections::{BTreeMap, BTreeSet},
    string::String,
    sync::Arc,
};
use cow_arc::CowArc;
use crate_metadata::*;
use fs_node::FileRef;
use hashbrown::HashMap;
use memory::{MappedPages, VirtualAddress};
use serde::{Deserialize, Serialize};
use spin::Mutex;

/// A (de)serializable representation of a loaded crate that is `serde`-compatible.
///
/// See [`LoadedCrate`] for more detail on the fields of this struct.
#[derive(Debug, Serialize, Deserialize)]
pub struct SerializedCrate {
    /// The name of the crate.
    pub crate_name: String,
    /// A map containing all the sections of the crate.
    pub sections: HashMap<Shndx, SerializedSection>,
    /// A set containing the global sectinos of the crate.
    pub global_sections: BTreeSet<Shndx>,
    /// A set containing the thread-local storage (TLS) sections of the crate.
    pub tls_sections: BTreeSet<Shndx>,
    /// A set containing the `.data` and `.bss` sections of the crate.
    pub data_sections: BTreeSet<Shndx>,
    /// A map of symbol names to their constant values, which contain assembler and linker
    /// constants.
    pub init_symbols: BTreeMap<String, usize>,
}

impl SerializedCrate {
    /// Convert this into a [`LoadedCrate`] and its sections into [`LoadedSection`]s.
    pub(crate) fn into_loaded_crate(
        self,
        object_file: FileRef,
        namespace: &Arc<CrateNamespace>,
        text_pages: &Arc<Mutex<MappedPages>>,
        rodata_pages: &Arc<Mutex<MappedPages>>,
        data_pages: &Arc<Mutex<MappedPages>>,
        verbose_log: bool,
    ) -> Result<(StrongCrateRef, BTreeMap<String, usize>, usize), &'static str> {
        let crate_name: StrRef = self.crate_name.as_str().into();
        
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
            global_sections:     self.global_sections,
            tls_sections:        self.tls_sections,
            data_sections:       self.data_sections,
            reexported_symbols:  BTreeSet::new(),
        });

        let mut sections = HashMap::with_capacity(self.sections.len());
        for (shndx, section) in self.sections {
            sections.insert(
                shndx,
                section.into_loaded_section(
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
        namespace
            .crate_tree
            .lock()
            .insert(crate_name, loaded_crate.clone_shallow());
        info!(
            "Finished parsing nano_core crate, added {} new symbols.",
            num_new_syms
        );
        
        // // Dump loaded sections for verification. See pull request #542/#559 for more details:
        // let loaded_crate_ref = loaded_crate.lock_as_ref();
        // for (_, section) in loaded_crate_ref.sections.iter() {
        //     trace!("{:016x} {} {}", section.address_range.start.value(), section.name, section.mapped_pages_offset);
        // }
        // drop(loaded_crate_ref);

        Ok((loaded_crate, self.init_symbols, num_new_syms))
    }
}

/// A (de)serializable representation of a loaded section that is `serde`-compatible.
///
/// See [`LoadedSection`] for more detail on the fields of this struct.
#[derive(Debug, Serialize, Deserialize)]
pub struct SerializedSection {
    /// The full name of the section.
    pub name: String,
    /// The type of the section.
    pub ty: SectionType,
    /// Whether or not the section is global.
    pub global: bool,
    /// The starting [`VirtualAddress`] of the range covered by this section.
    pub virtual_address: usize,
    /// The offset into the [`MappedPages`] where this section starts.
    pub offset: usize,
    /// The size of the section.
    pub size: usize,
}

impl SerializedSection {
    /// Convert this into a [`LoadedSection`].
    pub(crate) fn into_loaded_section(
        self,
        parent_crate: WeakCrateRef,
        namespace: &Arc<CrateNamespace>,
        text_pages: &Arc<Mutex<MappedPages>>,
        rodata_pages: &Arc<Mutex<MappedPages>>,
        data_pages: &Arc<Mutex<MappedPages>>,
    ) -> Result<Arc<LoadedSection>, &'static str> {
        let mapped_pages = match self.ty {
            SectionType::Text => Arc::clone(text_pages),
            SectionType::Rodata
            | SectionType::TlsData
            | SectionType::TlsBss
            | SectionType::GccExceptTable
            | SectionType::EhFrame => Arc::clone(rodata_pages),
            SectionType::Data | SectionType::Bss => Arc::clone(data_pages),
        };
        let virtual_address = VirtualAddress::new(self.virtual_address)
            .ok_or("SerializedSection::into_loaded_section(): invalid virtual address")?;

        let loaded_section = Arc::new(LoadedSection {
            name: match self.ty {
                SectionType::EhFrame
                | SectionType::GccExceptTable => self.ty.name_str_ref(),
                _ => self.name.as_str().into(),
            },
            typ: self.ty,
            global: self.global,
            mapped_pages_offset: self.offset,
            mapped_pages,
            address_range: virtual_address..(virtual_address + self.size),
            parent_crate,
            inner: Default::default(),
        });

        if let SectionType::TlsData | SectionType::TlsBss = self.ty {
            namespace
                .tls_initializer
                .lock()
                .add_existing_static_tls_section(
                    // TLS sections encode their TLS offset in the virtual address field,
                    // which is necessary to properly calculate relocation entries that depend upon them.
                    self.virtual_address,
                    Arc::clone(&loaded_section),
                )
                .unwrap();
        }

        Ok(loaded_section)
    }
}
