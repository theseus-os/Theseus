use std::{
    env,
    fs::{self, File},
    io::{Read, Seek, SeekFrom, Write},
    path::PathBuf,
};

use goblin::{
    container::{Container, Ctx, Endian},
    elf::{
        header::header64::Header,
        section_header::SHT_SYMTAB,
        sym::{Symtab, STT_HIOS, STT_LOOS},
        SectionHeader,
    },
    elf64::{
        header::EI_DATA,
        section_header::{SHF_MASKOS, SHF_TLS},
        sym::STT_TLS,
    },
    strtab::Strtab,
};

/// The flag identifying CLS sections.
///
/// This must be kept in sync with `mod_mgmt`.
const CLS_SECTION_FLAG: u64 = 0x100000;

const _: () = assert!(CLS_SECTION_FLAG & SHF_MASKOS as u64 == CLS_SECTION_FLAG);

/// The flag identifying CLS symbols.
///
/// This must be kept in sync with `mod_mgmt`.
const CLS_SYMBOL_TYPE: u8 = 0xa;

const _: () = assert!(CLS_SYMBOL_TYPE >= STT_LOOS);
const _: () = assert!(CLS_SYMBOL_TYPE <= STT_HIOS);

// TODO: Cleanup and document.

fn main() {
    let file_path = env::args().next_back().expect("no file path provided");

    let mut file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&file_path)
        .expect("failed to open file");
    let header = Header::from_fd(&mut file).unwrap();

    let sections = sections(&header, &mut file);
    let did_update = update_cls_section(&header, &sections, &mut file);
    if did_update {
        println!(
            "detected .cls section in {}",
            PathBuf::from(file_path)
                .file_name()
                .unwrap()
                .to_string_lossy(),
        );
        update_cls_symbols(&header, &sections, &mut file);
    }
}

fn sections(header: &Header, file: &mut File) -> Vec<SectionHeader> {
    let le = match header.e_ident[EI_DATA] {
        1 => Endian::Little,
        2 => Endian::Big,
        _ => panic!(),
    };

    let context = Ctx {
        container: Container::Big,
        le,
    };

    // `SectionHeader::parse` will instantly return if the offset is 0, so we trick
    // it by loading an extra byte before the list of section headers.
    let mut bytes = vec![0; header.e_shnum as usize * header.e_shentsize as usize + 1];
    file.seek(SeekFrom::Start(header.e_shoff - 1)).unwrap();
    file.read_exact(&mut bytes).unwrap();
    SectionHeader::parse(&bytes, 1, header.e_shnum as usize, context).unwrap()
}

fn update_cls_section(header: &Header, sections: &[SectionHeader], file: &mut File) -> bool {
    let section_header_string_table_index = header.e_shstrndx;
    let string_table_section = &sections[section_header_string_table_index as usize];

    let mut string_table_bytes = vec![0; string_table_section.sh_size as usize];
    file.seek(SeekFrom::Start(string_table_section.sh_offset))
        .unwrap();
    file.read_exact(&mut string_table_bytes).unwrap();
    let string_table = Strtab::new(&string_table_bytes, 0);

    for (i, section) in sections.iter().enumerate() {
        let name = string_table.get_unsafe(section.sh_name).unwrap();
        if name == ".cls" {
            // The flag variable is bytes 8-16 of the section header.
            let flag_position = header.e_shoff + i as u64 * header.e_shentsize as u64 + 8;
            file.seek(SeekFrom::Start(flag_position)).unwrap();
            // TODO: Le, be
            let mut flag_bytes = [0; 8];
            file.read_exact(&mut flag_bytes).unwrap();
            let mut flags = u64::from_le_bytes(flag_bytes);

            // Sanity check that the TLS flag is set.
            assert_ne!(
                flags & SHF_TLS as u64,
                0,
                "TLS flag not set for .cls section"
            );
            // Unset the TLS flag.
            flags &= !SHF_TLS as u64;
            // Set the CLS flag.
            flags |= CLS_SECTION_FLAG;

            // Overwrite the old flags.
            file.seek(SeekFrom::Current(-8)).unwrap();
            file.write_all(&flags.to_le_bytes())
                .expect("failed to write .cls section type to file");

            return true;
        }
    }
    false
}

fn update_cls_symbols(header: &Header, sections: &[SectionHeader], file: &mut File) {
    let section = sections
        .iter()
        .find(|section| section.sh_type == SHT_SYMTAB)
        .unwrap();
    let section_offset = section.sh_offset;
    let section_size = section.sh_size;
    let entry_size = section.sh_entsize;
    let symbol_count = if entry_size == 0 {
        0
    } else {
        section_size / entry_size
    };

    let mut section_bytes = vec![0; section_size as usize];
    file.seek(SeekFrom::Start(section_offset)).unwrap();
    file.read_exact(&mut section_bytes).unwrap();

    let le = match header.e_ident[EI_DATA] {
        1 => Endian::Little,
        2 => Endian::Big,
        _ => panic!(),
    };

    let context = Ctx {
        container: Container::Big,
        le,
    };

    let symbol_table = Symtab::parse(&section_bytes, 0, symbol_count as usize, context).unwrap();

    for (i, symbol) in symbol_table.iter().enumerate() {
        let ty = symbol.st_info & 0xf;
        if ty == STT_TLS {
            let new_info = (symbol.st_info & 0xf0) | CLS_SYMBOL_TYPE;

            let type_offset = section_offset + i as u64 * entry_size + 4;
            file.seek(SeekFrom::Start(type_offset)).unwrap();
            file.write_all(&[new_info]).unwrap();
            println!("overwriting CLS symbol flag");
        }
    }
}
