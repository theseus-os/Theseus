use std::{
    env,
    error::Error,
    fs,
    io::{Read, Seek, SeekFrom, Write},
    iter,
    path::PathBuf,
};

use goblin::{
    container::{Container, Ctx, Endian},
    elf::{header::header64::Header, SectionHeader},
    elf64::{
        header::EI_DATA,
        section_header::{SHF_MASKOS, SHF_TLS},
    },
    strtab::Strtab,
};

/// The flag identifying CLS sections.
///
/// This must be kept in sync with `mod_mgmt`.
const CLS_SECTION_FLAG: u64 = 0x100000;
const _: () = assert!(CLS_SECTION_FLAG & SHF_MASKOS as u64 == CLS_SECTION_FLAG);

// TODO: Cleanup and document.

fn main() {
    let file_path = env::args().next_back().expect("no file path provided");

    let mut file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&file_path)
        .expect("failed to open file");
    let header = Header::from_fd(&mut file).unwrap();

    let section_header_string_table_index = header.e_shstrndx;

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
    let mut section_header_bytes =
        vec![0; header.e_shnum as usize * header.e_shentsize as usize + 1];
    file.seek(SeekFrom::Start(header.e_shoff - 1)).unwrap();
    file.read_exact(&mut section_header_bytes).unwrap();
    let sections =
        SectionHeader::parse(&section_header_bytes, 1, header.e_shnum as usize, context).unwrap();

    let string_table_section = &sections[section_header_string_table_index as usize];

    let mut string_table_bytes = vec![0; string_table_section.sh_size as usize];
    file.seek(SeekFrom::Start(string_table_section.sh_offset))
        .unwrap();
    file.read_exact(&mut string_table_bytes).unwrap();
    let string_table = Strtab::new(&string_table_bytes, 0);

    for (i, section) in sections.into_iter().enumerate() {
        let name = string_table.get_unsafe(section.sh_name).unwrap();
        if name == ".cls" {
            println!(
                "setting CLS flag in {}",
                PathBuf::from(file_path)
                    .file_name()
                    .unwrap()
                    .to_string_lossy(),
            );

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
            file.write(flags).to_le_bytes())
                .expect("failed to write .cls section type to file");

            return;
        }
    }
}
