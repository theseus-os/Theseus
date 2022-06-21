use crate_metadata::{LoadedCrate, SectionType, Shndx};
use hashbrown::HashMap;
use log::{error, trace};
use memory::VirtualAddress;
use mod_mgmt::serde::{SerializedCrate, SerializedSection};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::BufRead,
    sync::Arc,
};

/// Parses the nano_core symbol file that represents the already loaded (and currently running) nano_core code.
/// Basically, just searches the section list for offsets, size, and flag data,
/// and parses the symbol table to populate the list of sections.
pub fn parse_nano_core_symbol_file(symbol_str: String) -> Result<ParsedCrateItems, &'static str> {
    let mut text_shndx: Option<Shndx> = None;
    let mut rodata_shndx: Option<Shndx> = None;
    let mut data_shndx: Option<Shndx> = None;
    let mut bss_shndx: Option<Shndx> = None;
    let mut tls_data_shndx: Option<(Shndx, usize)> = None;
    let mut tls_bss_shndx: Option<(Shndx, usize)> = None;

    /// An internal function that parses a section header's index (e.g., "[7]") out of the given str.
    /// Returns a tuple of the parsed `Shndx` and the rest of the unparsed str after the shndx.
    fn parse_section_ndx(str_ref: &str) -> Option<(Shndx, &str)> {
        let open = str_ref.find("[");
        let close = str_ref.find("]");
        open.and_then(|start| {
            close.and_then(|end| {
                str_ref
                    .get(start + 1..end)
                    .and_then(|t| t.trim().parse::<usize>().ok())
                    .and_then(|shndx| str_ref.get(end + 1..).map(|the_rest| (shndx, the_rest)))
            })
        })
    }

    /// An internal function that parses a section header's address and size
    /// from a string that starts at the `Name` field of a line.
    fn parse_section_vaddr_size(sec_hdr_line_starting_at_name: &str) -> Option<(usize, usize)> {
        let mut tokens = sec_hdr_line_starting_at_name.split_whitespace();
        tokens.next(); // skip Name
        tokens.next(); // skip Type
        let addr_hex_str = tokens.next();
        tokens.next(); // skip Off (offset)
        let size_hex_str = tokens.next();
        // parse both the Address and Size fields as hex strings
        addr_hex_str
            .and_then(|a| usize::from_str_radix(a, 16).ok())
            // .and_then(VirtualAddress::new)
            .and_then(|vaddr| {
                size_hex_str
                    .and_then(|s| usize::from_str_radix(s, 16).ok())
                    .map(|size| (vaddr, size))
            })
    }

    // We will fill in these crate items while parsing the symbol file.
    let mut crate_items = ParsedCrateItems::empty();
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

        if line.contains(".text ") && line.contains("PROGBITS") {
            text_shndx = parse_section_ndx(line).map(|(shndx, _)| shndx);
        } else if line.contains(".rodata ") && line.contains("PROGBITS") {
            rodata_shndx = parse_section_ndx(line).map(|(shndx, _)| shndx);
        } else if line.contains(".tdata ") && line.contains("PROGBITS") {
            tls_data_shndx = parse_section_ndx(line).and_then(|(shndx, rest_of_line)| {
                parse_section_vaddr_size(rest_of_line).map(|(vaddr, _)| (shndx, vaddr))
            });
        } else if line.contains(".tbss ") && line.contains("NOBITS") {
            tls_bss_shndx = parse_section_ndx(line).and_then(|(shndx, rest_of_line)| {
                parse_section_vaddr_size(rest_of_line).map(|(vaddr, _)| (shndx, vaddr))
            });
        } else if line.contains(".data ") && line.contains("PROGBITS") {
            data_shndx = parse_section_ndx(line).map(|(shndx, _)| shndx);
        } else if line.contains(".bss ") && line.contains("NOBITS") {
            bss_shndx = parse_section_ndx(line).map(|(shndx, _)| shndx);
        } else if let Some(start) = line.find(".eh_frame ") {
            let (virtual_address, size) = parse_section_vaddr_size(&line[start..])
                .ok_or("Failed to parse the .eh_frame section header's address and size")?;

            crate_items.sections.insert(
                section_counter,
                SerializedSection {
                    name: String::from(".eh_frame"),
                    ty: SectionType::EhFrame,
                    global: false, // .eh_frame is not global
                    virtual_address,
                    offset: virtual_address,
                    size,
                },
            );

            section_counter += 1;
        } else if let Some(start) = line.find(".gcc_except_table ") {
            let (virtual_address, size) = parse_section_vaddr_size(&line[start..])
                .ok_or("Failed to parse the .gcc_except_table section header's address and size")?;

            crate_items.sections.insert(
                section_counter,
                SerializedSection {
                    name: String::from(".gcc_except_table"),
                    ty: SectionType::GccExceptTable,
                    global: false, // .gcc_except_table is not global
                    virtual_address,
                    offset: virtual_address,
                    size,
                },
            );

            section_counter += 1;
        }
    }

    let text_shndx =
        text_shndx.ok_or("parse_nano_core_symbol_file(): couldn't find .text section index")?;
    let rodata_shndx =
        rodata_shndx.ok_or("parse_nano_core_symbol_file(): couldn't find .rodata section index")?;
    let data_shndx =
        data_shndx.ok_or("parse_nano_core_symbol_file(): couldn't find .data section index")?;
    let bss_shndx =
        bss_shndx.ok_or("parse_nano_core_symbol_file(): couldn't find .bss section index")?;
    let shndxs = MainShndx {
        text_shndx,
        rodata_shndx,
        data_shndx,
        bss_shndx,
        tls_data_shndx,
        tls_bss_shndx,
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
                error!("parse_nano_core_symbol_file(): error parsing virtual address Value at line {}: {:?}\n    line: {}", _line_num + 1, e, line);
                "parse_nano_core_symbol_file(): couldn't parse virtual address (value column)"
            })?;
            let sec_size = usize::from_str_radix(sec_size, 10).or_else(|e| {
                sec_size.get(2 ..).ok_or(e).and_then(|sec_size_hex| usize::from_str_radix(sec_size_hex, 16))
            }).map_err(|e| {
                error!("parse_nano_core_symbol_file(): error parsing size at line {}: {:?}\n    line: {}", _line_num + 1, e, line);
                "parse_nano_core_symbol_file(): couldn't parse size column"
            })?;

            // while vaddr and size are required, ndx could be valid or not.
            let sec_ndx = match usize::from_str_radix(sec_ndx, 10) {
                // If ndx is a valid number, proceed on.
                Ok(ndx) => ndx,
                // Otherwise, if ndx is not a number (e.g., "ABS"), then we just skip that entry (go onto the next line).
                _ => {
                    trace!(
                        "parse_nano_core_symbol_file(): skipping line {}: {}",
                        _line_num + 1,
                        line
                    );
                    continue;
                }
            };

            // debug!("parse_nano_core_symbol_file(): name: {}, vaddr: {:#X}, size: {:#X}, sec_ndx {}", name, sec_vaddr, sec_size, sec_ndx);

            add_new_section(
                &shndxs,
                &mut crate_items,
                &mut section_counter,
                sec_ndx,
                String::from(name),
                sec_size,
                sec_vaddr,
                global,
            )?;
        } // end of loop over all lines
    }

    trace!("parse_nano_core_symbol_file(): finished looping over symtab.");
    Ok(crate_items)
}

/// The collection of sections and symbols obtained while parsing the nano_core crate.
pub struct ParsedCrateItems {
    pub sections: HashMap<Shndx, SerializedSection>,
    pub global_sections: BTreeSet<Shndx>,
    pub data_sections: BTreeSet<Shndx>,
    /// The set of other non-section symbols too, such as constants defined in assembly code.
    pub init_symbols: BTreeMap<String, usize>,
}

impl ParsedCrateItems {
    fn empty() -> ParsedCrateItems {
        ParsedCrateItems {
            sections: HashMap::new(),
            global_sections: BTreeSet::new(),
            data_sections: BTreeSet::new(),
            init_symbols: BTreeMap::new(),
        }
    }
}

/// The section header indices (shndx) for the main sections:
/// .text, .rodata, .data, and .bss.
///
/// If TLS sections are present, e.g., .tdata or .tbss,
/// their `shndx`s and starting virtul addresses are also included here.
struct MainShndx {
    text_shndx: Shndx,
    rodata_shndx: Shndx,
    data_shndx: Shndx,
    bss_shndx: Shndx,
    tls_data_shndx: Option<(Shndx, usize)>,
    tls_bss_shndx: Option<(Shndx, usize)>,
}

/// A convenience function that separates out the logic
/// of actually creating and adding a new LoadedSection instance
/// after it has been parsed.
fn add_new_section(
    shndxs: &MainShndx,
    crate_items: &mut ParsedCrateItems,
    section_counter: &mut Shndx,
    // crate-wide args above, section-specific stuff below
    sec_ndx: Shndx,
    name: String,
    size: usize,
    virtual_address: usize,
    global: bool,
) -> Result<(), &'static str> {
    let new_section = if sec_ndx == shndxs.text_shndx {
        // let virtual_address = VirtualAddress::new(virtual_address)
        //     .ok_or("new text section had invalid virtual address")?;

        Some(SerializedSection {
            name,
            ty: SectionType::Text,
            global,
            virtual_address,
            offset: virtual_address,
            size,
        })
    } else if sec_ndx == shndxs.rodata_shndx {
        // let virtual_address = VirtualAddress::new(virtual_address)
        //     .ok_or("new rodata section had invalid virtual address")?;

        Some(SerializedSection {
            name,
            ty: SectionType::Rodata,
            global,
            virtual_address,
            offset: virtual_address,
            size,
        })
    } else if sec_ndx == shndxs.data_shndx {
        // let virtual_address = VirtualAddress::new(virtual_address)
        //     .ok_or("new data section had invalid virtual address")?;

        Some(SerializedSection {
            name,
            ty: SectionType::Data,
            global,
            virtual_address,
            offset: virtual_address,
            size,
        })
    } else if sec_ndx == shndxs.bss_shndx {
        // let virtual_address = VirtualAddress::new(virtual_address)
        //     .ok_or("new bss section had invalid virtual address")?;

        Some(SerializedSection {
            name,
            ty: SectionType::Bss,
            global,
            virtual_address,
            offset: virtual_address,
            size,
        })
    } else if shndxs
        .tls_data_shndx
        .map_or(false, |(shndx, _)| sec_ndx == shndx)
    {
        // TLS sections encode their TLS offset in the virtual address field,
        // which is necessary to properly calculate relocation entries that depend upon them.
        let tls_offset = virtual_address;
        // We do need to calculate the real virtual address so we can use that
        // to calculate the real mapped_pages_offset where its data exists.
        // so we can use that to calculate the real virtual address where it's loaded.
        let tls_sec_data_vaddr = shndxs.tls_data_shndx.unwrap().1 + tls_offset;

        dbg!(
            "tls data",
            &name,
            global,
            tls_sec_data_vaddr,
            size,
            virtual_address
        );

        Some(SerializedSection {
            name,
            ty: SectionType::TlsData,
            global,
            virtual_address: tls_offset,
            offset: tls_sec_data_vaddr,
            size,
        })
    } else if shndxs
        .tls_bss_shndx
        .map_or(false, |(shndx, _)| sec_ndx == shndx)
    {
        dbg!("tls bss", &name, global, virtual_address, size);
        Some(SerializedSection {
            name,
            ty: SectionType::TlsBss,
            global,
            // virtual_address: VirtualAddress::new(virtual_address)
            //     .ok_or("new TLS .tbss section had invalid virtual address (TLS offset)")?,
            virtual_address,
            offset: virtual_address,
            size,
        })
    } else {
        crate_items.init_symbols.insert(name, virtual_address);
        None
    };

    if let Some(sec) = new_section {
        // debug!("parse_nano_core: new section: {:?}", sec);
        if sec.global {
            crate_items.global_sections.insert(*section_counter);
        }
        if sec.ty.is_data_or_bss() {
            crate_items.data_sections.insert(*section_counter);
        }
        crate_items.sections.insert(*section_counter, sec);
        *section_counter += 1;
    }

    Ok(())
}
