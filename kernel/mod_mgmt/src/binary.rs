use crate::*;
use xmas_elf::header::Type;

impl CrateNamespace {
    pub fn load_executable_crate(
        namespace: &Arc<CrateNamespace>,
        crate_object_file_ref: &FileRef,
        kernel_mmi_ref: &MmiRef,
        verbose_log: bool,
    ) -> Result<AppCrateRef, &'static str> {
        debug!("loading crate as binary");
        let crate_object_file = crate_object_file_ref.lock();

        let mapped_pages = crate_object_file.as_mapping()?;
        let size_in_bytes = crate_object_file.len();

        let abs_path = Path::new(crate_object_file.get_absolute_path());
        let crate_name = StrRef::from(crate_name_from_path(&abs_path));

        let byte_slice: &[u8] = mapped_pages.as_slice(0, size_in_bytes)?;
        let elf_file = ElfFile::new(byte_slice)?;

        if elf_file.header.pt2.type_().as_type() != Type::Executable {
            return Err("tried to load non executable elf as executable");
        }

        // TODO: I think this should handle .init and .fini just fine.
        let section_pages = allocate_section_pages(&elf_file, kernel_mmi_ref)?;
        let text_pages = section_pages
            .executable_pages
            .map(|(tp, range)| (Arc::new(Mutex::new(tp)), range));
        let rodata_pages = section_pages
            .read_only_pages
            .map(|(rp, range)| (Arc::new(Mutex::new(rp)), range));
        let data_pages = section_pages
            .read_write_pages
            .map(|(dp, range)| (Arc::new(Mutex::new(dp)), range));

        CowArc::new(LoadedCrate {
            crate_name,
            debug_symbols_file: Arc::downgrade(&crate_object_file_ref),
            object_file: crate_object_file_ref.clone(),
            sections: HashMap::new(),
            text_pages: text_pages.clone(),
            rodata_pages: rodata_pages.clone(),
            data_pages: data_pages.clone(),
            global_sections: BTreeSet::new(),
            tls_sections: BTreeSet::new(),
            data_sections: BTreeSet::new(),
            reexported_symbols: BTreeSet::new(),
        });

        todo!();
    }
}
