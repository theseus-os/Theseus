use crate_metadata::{SectionType, Shndx};
use hashbrown::HashMap;
use mod_mgmt::serde::SerializedSection;
use std::collections::{BTreeMap, BTreeSet};

/// Parses the nano_core symbol file that represents the sections and symbols
/// in the initially running code (the kernel base image, i.e., "nano_core"),
/// which are loaded into the running system by the bootloader.
///
/// Basically, this parses the section list for offsets, size, and flag data,
/// and parses the symbol table to populate the list of sections.
pub fn parse_nano_core_symbol_file(symbol_str: String) -> Result<ParsedCrateItems, &'static str> {
    // We don't care about the .init sections shndx.
    let mut init_vaddr: Option<usize> = None;
    let mut text: Option<(Shndx, usize)> = None;
    let mut rodata: Option<(Shndx, usize)> = None;
    let mut data: Option<(Shndx, usize)> = None;
    // .bss is part of .data, so we don't need its vaddr
    let mut bss: Option<Shndx> = None;
    let mut tls_data: Option<(Shndx, usize)> = None; 
    // .tbss does not exist anywhere in memory, so we don't need its vaddr
    let mut tls_bss: Option<Shndx> = None;

    /// An internal function that parses a section header's index, address and size.
    fn parse_section(str_ref: &str) -> Option<(Shndx, usize, usize)> {
        let open = str_ref.find('[');
        let close = str_ref.find(']');
        let (shndx, the_rest) = open.and_then(|start| {
            close.and_then(|end| {
                str_ref
                    .get(start + 1..end)
                    .and_then(|t| t.trim().parse::<usize>().ok())
                    .and_then(|shndx| str_ref.get(end + 1..).map(|the_rest| (shndx, the_rest)))
            })
        })?;

        let mut tokens = the_rest.split_whitespace();
        tokens.next(); // skip Name
        tokens.next(); // skip Type
        let addr_hex_str = tokens.next();
        tokens.next(); // skip Off (offset)
        let size_hex_str = tokens.next();
        // parse both the Address and Size fields as hex strings
        let (vaddr, size) = addr_hex_str
            .and_then(|a| usize::from_str_radix(a, 16).ok())
            // .and_then(VirtualAddress::new)
            .and_then(|vaddr| {
                size_hex_str
                    .and_then(|s| usize::from_str_radix(s, 16).ok())
                    .map(|size| (vaddr, size))
            })?;
        
        Some((shndx, vaddr, size))
    }

    // We will fill in these crate items while parsing the symbol file.
    let mut crate_items = ParsedCrateItems::default();
    // As the nano_core doesn't have one section per function/data/rodata, we fake it here with an arbitrary section counter
    let mut section_counter = 0;

    // First, find the section indices that we care about: .text, .data, .rodata, .bss,
    // and also .eh_frame and .gcc_except_table, which are handled specially.
    // The reason we first look for the section indices is because we create
    // individual sections per symbol instead of one for each of those four sections,
    // which is how normal Rust crates are built and loaded (one section per symbol).
    let file_iterator = symbol_str.lines().enumerate();
    for (_line_num, line) in file_iterator.clone() {
        // skip empty lines
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // debug!("Looking at line: {:?}", line);

        if line.contains(".init") && line.contains("PROGBITS") {
            init_vaddr = parse_section(line).map(|(_, vaddr, _)| vaddr);  
        } else if line.contains(".text ") && line.contains("PROGBITS") {
            text = parse_section(line).map(|(shndx, _, _)| {
                (shndx, kernel_config::memory::KERNEL_OFFSET + init_vaddr.expect(".text parsed before .init"))
            });
        } else if line.contains(".rodata ") && line.contains("PROGBITS") {
            rodata = parse_section(line).map(|(shndx, vaddr, _)| (shndx, vaddr));
        } else if line.contains(".tdata ") && line.contains("PROGBITS") {
            tls_data = parse_section(line).map(|(shndx, vaddr, _)| (shndx, vaddr));
        } else if line.contains(".tbss ") && line.contains("NOBITS") {
            tls_bss = parse_section(line).map(|(shndx, ..)| shndx);
        } else if line.contains(".data ") && line.contains("PROGBITS") {
            data = parse_section(line).map(|(shndx, vaddr, _)| (shndx, vaddr));
        } else if line.contains(".bss ") && line.contains("NOBITS") {
            bss = parse_section(line).map(|(shndx, ..)| shndx);
        } else if line.contains(".eh_frame ") && line.contains("X86_64_UNWIND") {
            let (_, virtual_address, size) = parse_section(line)
                .ok_or("Failed to parse the .eh_frame section header's address and size")?;

            crate_items.sections.insert(
                section_counter,
                SerializedSection {
                    // The name gets set to EH_FRAME_STR_REF when loading the section.
                    name: String::new(),
                    ty: SectionType::EhFrame,
                    global: false, // .eh_frame is not global
                    virtual_address,
                    offset: virtual_address - rodata.expect(".eh_frame parsed before .rodata").1,
                    size,
                },
            );

            section_counter += 1;
        } else if line.contains(".gcc_except_table") && line.contains("PROGBITS") {
            let (_, virtual_address, size) = parse_section(line)
                .ok_or("Failed to parse the .gcc_except_table section header's address and size")?;

            crate_items.sections.insert(
                section_counter,
                SerializedSection {
                    // The name gets set to GCC_EXCEPT_TABLE_STR_REF when loading the section.
                    name: String::new(),
                    ty: SectionType::GccExceptTable,
                    global: false, // .gcc_except_table is not global
                    virtual_address,
                    offset: virtual_address - rodata.expect(".gcc_except_table parsed before .rodata").1,
                    size,
                },
            );

            section_counter += 1;
        }
    }

    let text =
        text.ok_or("parse_nano_core_symbol_file(): couldn't find .text section index")?;
    let rodata =
        rodata.ok_or("parse_nano_core_symbol_file(): couldn't find .rodata section index")?;
    let data =
        data.ok_or("parse_nano_core_symbol_file(): couldn't find .data section index")?;
    let bss =
        bss.ok_or("parse_nano_core_symbol_file(): couldn't find .bss section index")?;
    let shndxs = MainSections {
        text,
        rodata,
        data,
        bss,
        tls_data,
        tls_bss,
    };

    // second, skip ahead to the start of the symbol table: a line which contains ".symtab" but does NOT contain "SYMTAB"
    let is_start_of_symbol_table =
        |line: &str| line.contains(".symtab") && !line.contains("SYMTAB");
    let mut file_iterator =
        file_iterator.skip_while(|(_line_num, line)| !is_start_of_symbol_table(line));
    // skip the symbol table start line, e.g., "Symbol table '.symtab' contains N entries:"
    if let Some((_num, _line)) = file_iterator.next() {
        // trace!("SKIPPING LINE {}: {}", _num + 1, _line);
    }
    // skip one more line, the line with the column headers, e.g., "Num:     Value     Size Type   Bind   Vis ..."
    if let Some((_num, _line)) = file_iterator.next() {
        // trace!("SKIPPING LINE {}: {}", _num + 1, _line);
    }

    {
        // third, parse each symbol table entry
        for (_line_num, line) in file_iterator {
            if line.is_empty() {
                continue;
            }

            // we need the following items from a symbol table entry:
            // * Value (address),      column 1
            // * Size,                 column 2
            // * Bind (visibility),    column 4
            // * Ndx,                  column 6
            // * DemangledName#hash    column 7 to end

            // Can't use split_whitespace() here, because we need to splitn and then get the remainder of the line
            // after we've split the first 7 columns by whitespace. So we write a custom closure to group multiple whitespaces together.
            // We use "splitn(8, ..)" because it stops at the 8th column (column index 7) and gets the rest of the line in a single iteration.
            let mut prev_whitespace = true; // by default, we start assuming that the previous element was whitespace.
            let mut parts = line
                .splitn(8, |c: char| {
                    if c.is_whitespace() {
                        if prev_whitespace {
                            false
                        } else {
                            prev_whitespace = true;
                            true
                        }
                    } else {
                        prev_whitespace = false;
                        false
                    }
                })
                .map(str::trim);

            let _num = parts
                .next()
                .ok_or("parse_nano_core_symbol_file(): couldn't get column 0 'Num'")?;
            let sec_vaddr = parts
                .next()
                .ok_or("parse_nano_core_symbol_file(): couldn't get column 1 'Value'")?;
            let sec_size = parts
                .next()
                .ok_or("parse_nano_core_symbol_file(): couldn't get column 2 'Size'")?;
            let _typ = parts
                .next()
                .ok_or("parse_nano_core_symbol_file(): couldn't get column 3 'Type'")?;
            let bind = parts
                .next()
                .ok_or("parse_nano_core_symbol_file(): couldn't get column 4 'Bind'")?;
            let _vis = parts
                .next()
                .ok_or("parse_nano_core_symbol_file(): couldn't get column 5 'Vis'")?;
            let sec_ndx = parts
                .next()
                .ok_or("parse_nano_core_symbol_file(): couldn't get column 6 'Ndx'")?;
            let name = parts
                .next()
                .ok_or("parse_nano_core_symbol_file(): couldn't get column 7 'Name'")?;

            let global = bind == "GLOBAL" || bind == "WEAK";
            let sec_vaddr = usize::from_str_radix(sec_vaddr, 16).map_err(|e| {
                eprintln!("parse_nano_core_symbol_file(): error parsing virtual address Value at line {}: {:?}\n    line: {}", _line_num + 1, e, line);
                "parse_nano_core_symbol_file(): couldn't parse virtual address (value column)"
            })?;
            let sec_size = sec_size.parse::<usize>().or_else(|e| {
                sec_size.get(2 ..).ok_or(e).and_then(|sec_size_hex| usize::from_str_radix(sec_size_hex, 16))
            }).map_err(|e| {
                eprintln!("parse_nano_core_symbol_file(): error parsing size at line {}: {:?}\n    line: {}", _line_num + 1, e, line);
                "parse_nano_core_symbol_file(): couldn't parse size column"
            })?;

            // while vaddr and size are required, ndx could be valid or not.
            let sec_ndx = match sec_ndx.parse::<usize>() {
                // If ndx is a valid number, proceed on.
                Ok(ndx) => ndx,
                // Otherwise, if ndx is not a number (e.g., "ABS"), then we just skip that entry (go onto the next line).
                _ => {
                    // trace!(
                    //     "parse_nano_core_symbol_file(): skipping line {}: {}",
                    //     _line_num + 1,
                    //     line
                    // );
                    continue;
                }
            };
            
            // debug!("parse_nano_core_symbol_file(): name: {}, vaddr: {:#X}, size: {:#X}, sec_ndx {}", name, sec_vaddr, sec_size, sec_ndx);

            add_new_section(
                &shndxs,
                &mut crate_items,
                &mut section_counter,
                IndexMeta {
                    ndx: sec_ndx,
                    name: name.to_string(),
                    size: sec_size,
                    virtual_address: sec_vaddr,
                    global,
                },
            )?;
        } // end of loop over all lines
    }

    // trace!("parse_nano_core_symbol_file(): finished looping over symtab.");
    Ok(crate_items)
}

/// The collection of sections and symbols obtained while parsing the nano_core crate.
#[derive(Default)]
pub struct ParsedCrateItems {
    pub sections: HashMap<Shndx, SerializedSection>,
    pub global_sections: BTreeSet<Shndx>,
    pub tls_sections: BTreeSet<Shndx>,
    pub data_sections: BTreeSet<Shndx>,
    /// The set of other non-section symbols too, such as constants defined in assembly code.
    pub init_symbols: BTreeMap<String, usize>,
}

/// The section header indices (shndx) and starting virtual addresses for the main sections:
/// .text, .rodata, .data, .bss, .tdata, and .tbss, if they exist
struct MainSections {
    text: (Shndx, usize),
    rodata: (Shndx, usize),
    data: (Shndx, usize),
    bss: Shndx,
    tls_data: Option<(Shndx, usize)>,
    tls_bss: Option<Shndx>,
}

struct IndexMeta {
    ndx: Shndx,
    name: String,
    size: usize,
    virtual_address: usize,
    global: bool,
}

/// A convenience function that separates out the logic
/// of actually creating and adding a new LoadedSection instance
/// after it has been parsed.
fn add_new_section(
    main_sections: &MainSections,
    crate_items: &mut ParsedCrateItems,
    section_counter: &mut Shndx,
    meta: IndexMeta,
) -> Result<(), &'static str> {
    let IndexMeta {
        ndx: sec_ndx,
        name,
        size,
        virtual_address,
        global,
    } = meta;

    let new_section = if sec_ndx == main_sections.text.0 {
        Some(SerializedSection {
            name,
            ty: SectionType::Text,
            global,
            virtual_address,
            offset: virtual_address - main_sections.text.1,
            size,
        })
    } else if sec_ndx == main_sections.rodata.0 {
        Some(SerializedSection {
            name,
            ty: SectionType::Rodata,
            global,
            virtual_address,
            offset: virtual_address - main_sections.rodata.1,
            size,
        })
    } else if sec_ndx == main_sections.data.0 {
        Some(SerializedSection {
            name,
            ty: SectionType::Data,
            global,
            virtual_address,
            offset: virtual_address - main_sections.data.1,
            size,
        })
    } else if sec_ndx == main_sections.bss {
        Some(SerializedSection {
            name,
            ty: SectionType::Bss,
            global,
            virtual_address,
            // BSS sections are stored in the .data pages,
            // so the offset is from the start of the .data section.
             offset: virtual_address - main_sections.data.1,
            size,
        })
    } else if main_sections
        .tls_data
        .map_or(false, |(shndx, _)| sec_ndx == shndx)
    {
        // TLS sections encode their TLS offset in the virtual address field,
        // which is necessary to properly calculate relocation entries that depend upon them.
        let tls_offset = virtual_address;
        // We do need to calculate the real virtual address so we can use that
        // to calculate the real mapped_pages_offset where its data exists,
        // which we can then use to calculate the real virtual address where it's loaded.
        let tls_sec_data_vaddr = main_sections.tls_data.unwrap().1 + tls_offset;

        // The initial data image for .tdata sections exists in the rodata mapped pages
        let offset_from_rodata_start = tls_sec_data_vaddr - main_sections.rodata.1;

        Some(SerializedSection {
            name,
            ty: SectionType::TlsData,
            global,
            virtual_address: tls_offset,
            offset: offset_from_rodata_start,
            size,
        })
    } else if main_sections
        .tls_bss
        .map_or(false, |shndx| sec_ndx == shndx)
    {
        // TLS sections encode their TLS offset in the virtual address field,
        // which is necessary to properly calculate relocation entries that depend upon them.
        let tls_offset = virtual_address;
        
        // TLS BSS sections (.tbss) do not have any real loaded data in the ELF file,
        // since they are read-only initializer sections that would hold all zeroes.
        // Thus, we just use a max-value mapped pages offset as a canary value here,
        // as that value should never be used anyway.
        let canary_offset = usize::MAX;

        Some(SerializedSection {
            name,
            ty: SectionType::TlsBss,
            global,
            virtual_address: tls_offset,
            offset: canary_offset,
            size,
        })
    } else {
        crate_items.init_symbols.insert(name, virtual_address);
        None
    };

    if let Some(sec) = new_section {
        if sec.global {
            crate_items.global_sections.insert(*section_counter);
        }
        if sec.ty.is_data_or_bss() {
            crate_items.data_sections.insert(*section_counter);
        }
        if let SectionType::TlsData | SectionType::TlsBss = sec.ty {
            crate_items.tls_sections.insert(*section_counter);
        }
        crate_items.sections.insert(*section_counter, sec);
        *section_counter += 1;
    }

    Ok(())
}
