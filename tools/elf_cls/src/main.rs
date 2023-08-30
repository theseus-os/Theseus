//! Tool to prepare an ELF file with CLS sections.
//!
//! This involves replacing the `.cls` section's TLS flag with the CLS flag,
//! updating all CLS symbols to have the CLS type, and updating various symbol
//! values.
//!
//! Can be invoked on a single file
//! ```
//! elf_cls <arch> --file <path>
//! ```
//! or a directory of object files
//! ```
//! elf_cls <arch> --dir <path>
//! ```
#![feature(int_roundings)]

use std::{
    env,
    ffi::OsStr,
    fs::{self, File},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

use crate_metadata_serde::{CLS_SECTION_FLAG, CLS_SYMBOL_TYPE};
use goblin::{
    container::{Container, Ctx, Endian},
    elf::{
        header::header64::Header,
        section_header::SHT_SYMTAB,
        sym::{Symtab, STT_HIOS, STT_LOOS},
        SectionHeader,
    },
    elf64::{
        section_header::{SHF_MASKOS, SHF_TLS},
        sym::STT_TLS,
    },
    strtab::Strtab,
};

// Sanity check that the values we use are in the range reserved for custom OS shenanigans.
//
// These checks are here rather than in `crate_metadata_serde` to avoid an otherwise unnecessary
// dependency on `goblin`.
const _: () = assert!(CLS_SYMBOL_TYPE >= STT_LOOS);
const _: () = assert!(CLS_SYMBOL_TYPE <= STT_HIOS);
const _: () = assert!(CLS_SECTION_FLAG & SHF_MASKOS as u64 == CLS_SECTION_FLAG);

fn main() {
    let object_file_extension = OsStr::new("o");

    let mut args = env::args();
    args.next().unwrap();
    let is_x64 = match &args.next().unwrap()[..] {
        "x86_64" => true,
        "aarch64" => false,
        arch => panic!("invalid architecture: {arch}"),
    };
    match &args.next().unwrap()[..] {
        "--file" => {
            let file_path = args.next().unwrap();
            update_file(&file_path, is_x64);
        }
        "--dir" => {
            let directory_path = args.next().expect("no directory path provided");
            for entry in fs::read_dir(directory_path).unwrap() {
                let entry = entry.unwrap();
                let file_path = entry.path();
                if file_path.extension() == Some(object_file_extension) {
                    update_file(&file_path, is_x64);
                }
            }
        }
        option => panic!("invalid option: {option}"),
    }
}

fn sections(header: &Header, file: &mut File) -> Vec<SectionHeader> {
    let context = Ctx {
        container: Container::Big,
        le: Endian::Little,
    };

    // `SectionHeader::parse` will instantly return if the offset is 0, so we trick
    // it by loading an extra byte before the list of section headers.
    let mut bytes = vec![0; header.e_shnum as usize * header.e_shentsize as usize + 1];
    file.seek(SeekFrom::Start(header.e_shoff - 1)).unwrap();
    file.read_exact(&mut bytes).unwrap();
    SectionHeader::parse(&bytes, 1, header.e_shnum as usize, context).unwrap()
}

fn update_file<P>(file_path: P, is_x64: bool)
where
    P: AsRef<Path> + Copy,
    PathBuf: From<P>,
{
    if let Ok(mut file) = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(file_path)
    {
        let header = Header::from_fd(&mut file).unwrap();

        let sections = sections(&header, &mut file);
        if let Some((cls_section_index, cls_section_size, tls_section_size)) =
            update_cls_section(&header, &sections, &mut file)
        {
            // println!(
            //     "detected .cls section in {}",
            //     PathBuf::from(file_path)
            //         .file_name()
            //         .unwrap()
            //         .to_string_lossy(),
            // );
            update_cls_symbols(
                cls_section_index,
                cls_section_size,
                tls_section_size,
                &sections,
                &mut file,
                is_x64,
            );
        }
    }
}

/// Update the CLS section flag returning the CLS section index, CLS section
/// size, and TLS section size.
fn update_cls_section(
    header: &Header,
    sections: &[SectionHeader],
    file: &mut File,
) -> Option<(usize, u64, u64)> {
    let section_header_string_table_index = header.e_shstrndx;
    let string_table_section = &sections[section_header_string_table_index as usize];

    let mut string_table_bytes = vec![0; string_table_section.sh_size as usize];
    file.seek(SeekFrom::Start(string_table_section.sh_offset))
        .unwrap();
    file.read_exact(&mut string_table_bytes).unwrap();
    let string_table = Strtab::new(&string_table_bytes, 0);

    let mut cls_info = None;
    let mut tls_size = 0;

    for (i, section) in sections.iter().enumerate() {
        let name = string_table.get_unsafe(section.sh_name).unwrap();
        if name == ".cls" {
            let mut flags = section.sh_flags;

            if flags & CLS_SECTION_FLAG != 0 {
                // The tool is being rerun on a file.
                return None;
            }

            if flags & SHF_TLS as u64 == 0 {
                panic!("TLS flag not set for .cls section");
            }

            // Unset the TLS flag.
            flags &= !SHF_TLS as u64;
            // Set the CLS flag.
            flags |= CLS_SECTION_FLAG;

            // Overwrite the old flags.
            let flag_position = header.e_shoff + i as u64 * header.e_shentsize as u64 + 8;
            file.seek(SeekFrom::Start(flag_position)).unwrap();
            file.write_all(&flags.to_le_bytes()).unwrap();

            cls_info = Some((i, section.sh_size));
        } else {
            let is_tls = section.sh_flags & SHF_TLS as u64 == SHF_TLS as u64;
            if is_tls {
                tls_size += section.sh_size;
            }
        }
    }

    cls_info.map(|(cls_index, cls_size)| (cls_index, cls_size, tls_size))
}

fn update_cls_symbols(
    cls_section_index: usize,
    cls_size: u64,
    tls_size: u64,
    sections: &[SectionHeader],
    file: &mut File,
    is_x64: bool,
) {
    let symbol_table_section = sections
        .iter()
        .find(|section| section.sh_type == SHT_SYMTAB)
        .unwrap();
    let symbol_table_offset = symbol_table_section.sh_offset;
    let symbol_table_size = symbol_table_section.sh_size;
    let symbol_size = symbol_table_section.sh_entsize;
    let symbol_count = if symbol_size == 0 {
        0
    } else {
        symbol_table_size / symbol_size
    };

    let mut symbol_table_bytes = vec![0; symbol_table_size as usize];
    file.seek(SeekFrom::Start(symbol_table_offset)).unwrap();
    file.read_exact(&mut symbol_table_bytes).unwrap();

    let context = Ctx {
        container: Container::Big,
        le: Endian::Little,
    };

    let symbol_table =
        Symtab::parse(&symbol_table_bytes, 0, symbol_count as usize, context).unwrap();

    // CLS size cannot be zero, otherwise this function wouldn't have been called.
    let cls_rounded_size = cls_size.next_multiple_of(0x1000);
    let tls_rounded_size = if tls_size == 0 {
        0
    } else {
        tls_size.next_multiple_of(0x1000)
    };

    // The value of a CLS/TLS symbol is the offset into the CLS/TLS section.
    // Negative offsets are for relocations, **not** symbol values.

    for (i, symbol) in symbol_table.iter().enumerate() {
        if symbol.st_type() == STT_TLS {
            if symbol.st_shndx == cls_section_index {
                let new_info = (symbol.st_info & 0xf0) | CLS_SYMBOL_TYPE;
                let symbol_info_offset = symbol_table_offset + i as u64 * symbol_size + 4;
                file.seek(SeekFrom::Start(symbol_info_offset)).unwrap();
                file.write_all(&[new_info]).unwrap();

                // On AArch64, the CLS symbols have the wrong value. The linker thinks the data
                // image looks like
                // ```
                // +-----------------------+-----+-----------------------+------------------------+
                // | statically linked TLS | 000 | statically linked CLS | dynamically linked TLS |
                // +-----------------------+-----+-----------------------+------------------------+
                // ```
                // where `000` are padding bytes to align the start of the statically linked CLS
                // to a page boundary.
                //
                // So we subtract the size of the TLS section + padding bytes from all CLS
                // variables.
                if !is_x64 && symbol.st_value >= tls_rounded_size {
                    let new_value = symbol.st_value - tls_rounded_size;
                    let symbol_value_offset = symbol_table_offset + i as u64 * symbol_size + 8;
                    file.seek(SeekFrom::Start(symbol_value_offset)).unwrap();
                    file.write_all(&new_value.to_le_bytes()).unwrap();
                    // println!("overwrote CLS symbol value and flag");
                } else {
                    // println!("overwrote CLS symbol flag");
                }
            // On x64, the TLS symbols have the wrong value. The linker thinks
            // the data image looks like
            // ```
            // +-----------------------+-----+-----------------------+
            // | statically linked cls | 000 | statically linked TLS |
            // +-----------------------+-----+-----------------------+
            // ```
            // where `000` are padding bytes to align the start of the
            // statically linked TLS to a page boundary.
            //
            // So we subtract the size of the CLS section + padding bytes from
            // all TLS variables.
            } else if is_x64 && symbol.st_value >= cls_rounded_size {
                let new_value = symbol.st_value - cls_rounded_size;
                let symbol_value_offset = symbol_table_offset + i as u64 * symbol_size + 8;
                file.seek(SeekFrom::Start(symbol_value_offset)).unwrap();
                file.write_all(&new_value.to_le_bytes()).unwrap();
                // println!("overwrote TLS symbol value");
            }
        }
    }
}
