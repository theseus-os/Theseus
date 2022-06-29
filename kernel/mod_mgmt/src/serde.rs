//! Module containing structs necessary for serializing and deserializing crates.
//!
//! This is currently only used to parse the `nano_core` object file at compile time. After being
//! parsed, it is serialized into an instance of [`SerializedCrate`] and included as a boot module.

use crate::CrateNamespace;
use alloc::{
    collections::{BTreeMap, BTreeSet},
    string::String,
    sync::Arc,
};
use core::ops::Range;
use cow_arc::CowArc;
use crate_metadata::{
    LoadedCrate, LoadedSection, SectionType, Shndx, StrongCrateRef, WeakCrateRef,
};
use fs_node::FileRef;
use hashbrown::HashMap;
use memory::{MappedPages, VirtualAddress};
use serde::{Deserialize, Serialize};
use spin::Mutex;

/// A serialized representation of a crate.
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
    /// Load the crate and its sections.
    pub fn load(
        self,
        object_file: FileRef,
        namespace: &Arc<CrateNamespace>,
        text_pages: &Arc<Mutex<MappedPages>>,
        rodata_pages: &Arc<Mutex<MappedPages>>,
        data_pages: &Arc<Mutex<MappedPages>>,
        verbose_log: bool,
    ) -> Result<(StrongCrateRef, BTreeMap<String, usize>, usize), &'static str> {
        
        // The sections need a weak reference back to the loaded_crate, and so we first create
        // the loaded_crate so we have something to reference when loading the sections.
        let loaded_crate = CowArc::new(LoadedCrate {
            crate_name:          self.crate_name.clone(),
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
                section.load(
                    CowArc::downgrade(&loaded_crate),
                    namespace,
                    text_pages,
                    rodata_pages,
                    data_pages,
                )?,
            );
        }

        trace!(
            "SerializedCrate::load(): adding symbols to namespace {:?}...",
            namespace.name
        );
        let num_new_syms = namespace.add_symbols(sections.values(), verbose_log);
        trace!("SerializedCrate::load(): finished adding symbols.");

        let mut loaded_crate_mut = loaded_crate.lock_as_mut().ok_or(
            "BUG: SerializedCrate::load(): couldn't get exclusive mutable access to loaded_crate",
        )?;
        loaded_crate_mut.sections = sections;
        drop(loaded_crate_mut);

        // Add the newly-parsed nano_core crate to the kernel namespace.
        namespace
            .crate_tree
            .lock()
            .insert(self.crate_name.into(), loaded_crate.clone_shallow());
        info!(
            "Finished parsing nano_core crate, {} new symbols.",
            num_new_syms
        );

        Ok((loaded_crate, self.init_symbols, num_new_syms))
    }
}

/// A serialized representation of a section.
///
/// See [`LoadedSection`] for more detail on the fields of this struct.
#[derive(Debug, Serialize, Deserialize)]
pub struct SerializedSection {
    /// The full name of the section.
    pub name: String,
    /// The type of the scetion.
    pub ty: SectionType,
    /// Whether or not the section is global.
    pub global: bool,
    /// The starting [`VirtualAddress`] of the range covered by this section.
    pub virtual_address: usize,
    /// This field is identical to `self.virtual_address` unless `self.ty == SectionType::TlsData`.
    pub offset: usize,
    /// The size of the section.
    pub size: usize,
}

impl SerializedSection {
    pub fn load(
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
            .ok_or("SerializedSection::load(): invalid virtual address")?;

        let loaded_section = Arc::new(LoadedSection {
            name: self.name,
            typ: self.ty,
            global: self.global,
            // TLS BSS sections (.tbss) do not have any real loaded data in the ELF file,
            // since they are read-only initializer sections that would hold all zeroes.
            // Thus, we just use a max-value mapped pages offset as a canary value here,
            // as that value should never be used anyway.
            mapped_pages_offset: match self.ty {
                SectionType::TlsBss => usize::MAX,
                _ => mapped_pages
                    .lock()
                    .offset_of_address(
                        VirtualAddress::new(self.offset)
                            .ok_or("SerializedSection::load(): invalid offset")?,
                    )
                    .ok_or("nano_core section wasn't covered by its mapped pages")?,
            },
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

/// Convenience function for calculating the address range of a MappedPages object.
fn mp_range(mp_ref: &Arc<Mutex<MappedPages>>) -> Range<VirtualAddress> {
    let mp = mp_ref.lock();
    mp.start_address()..(mp.start_address() + mp.size_in_bytes())
}
