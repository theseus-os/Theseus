//! A tool that rewrites CPU-local storage accesses to use a different
//! register.
//!
//! Currently, the tool only supports x64, replacing `fs` with `gs`.

use goblin::elf::{reloc::R_X86_64_TPOFF32, Elf};
use std::{
    env,
    error::Error,
    fs,
    io::{Seek, SeekFrom, Write},
    iter,
};

fn main() -> Result<(), Box<dyn Error>> {
    let file_path = env::args().next_back().expect("no file path provided");

    let bytes = fs::read(&file_path)?;
    // TODO: Don't parse entire ELF.
    let elf = Elf::parse(&bytes)?;

    let mut cls_section_indexes =
        elf.section_headers
            .iter()
            .enumerate()
            .filter_map(|(index, header)| {
                if elf.shdr_strtab.get_at(header.sh_name).unwrap_or("") == ".cls" {
                    Some(index)
                } else {
                    None
                }
            });

    let Some(cls_section_index) = cls_section_indexes.next() else {
        return Ok(());
    };
    assert!(
        cls_section_indexes.next().is_none(),
        "multiple cls sections"
    );

    let cls_relocations = elf
        .shdr_relocs
        .iter()
        .flat_map(|(relocation_section_index, relocation_section)| {
            iter::repeat(*relocation_section_index).zip(relocation_section.iter())
        })
        .filter(|(_, relocation)| {
            elf.syms
                .get(relocation.r_sym)
                .expect("invalid relocation symbol table index")
                .st_shndx
                == cls_section_index
        });

    let mut file = fs::OpenOptions::new().write(true).open(file_path)?;

    for (relocation_section_index, relocation) in cls_relocations {
        // The target section is the one we are writing the relocation to.

        let target_section_index = elf.section_headers[relocation_section_index].sh_info as usize;
        let target_section_offset = elf.section_headers[target_section_index].sh_offset;

        // TODO: Support other architectures.
        match relocation.r_type {
            R_X86_64_TPOFF32 => {
                const FS_SEGMENT_PREFIX: u8 = 0x64;
                const GS_SEGMENT_PREFIX: u8 = 0x65;

                let in_section_offset = relocation.r_offset + 4;
                let offset = target_section_offset + in_section_offset;

                if bytes[offset as usize] == FS_SEGMENT_PREFIX {
                    file.seek(SeekFrom::Start(offset))?;
                    file.write_all(&[GS_SEGMENT_PREFIX])?;
                } else if bytes[offset as usize] == GS_SEGMENT_PREFIX {
                    // The tool is probably being rerun.
                } else {
                    panic!("invalid segment prefix");
                }
            }
            _ => todo!(),
        }
    }

    Ok(())
}
