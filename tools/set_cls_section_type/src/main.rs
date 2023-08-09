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
    elf64::header::EI_DATA,
    strtab::Strtab,
};

const CLS_SECTION_TYPE: u32 = 0x60000000;

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
                "rewriting .cls section type in {}",
                PathBuf::from(file_path)
                    .file_name()
                    .unwrap()
                    .to_string_lossy(),
            );
            file.seek(SeekFrom::Start(
                header.e_shoff + i as u64 * header.e_shentsize as u64 + 4,
            ))
            .unwrap();
            file.write(&CLS_SECTION_TYPE.to_le_bytes())
                .expect("failed to write .cls section type to file");
            return;
        }
    }
}
