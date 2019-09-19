//! Routines for parsing the gcc-style LSDA (Language-Specific Data Area) in an ELF object file, 
//! which corresponds to a part of the `.gcc_except_table` section. 

use gimli::{Reader, DwEhPe, EndianSlice, NativeEndian, constants::*, };

/// Parses the .gcc_except_table entries given that exist in the given `lsda` slice,
/// because LSDA (Language-specific data areas) are encoded according to the .gcc_except_table standards.
/// 
/// The flow of this code was partially inspired by rust's stdlib `libpanic_unwind/dwarf/eh.rs` file.
/// <https://github.com/rust-lang/rust/blob/master/src/libpanic_unwind/dwarf/eh.rs>
pub fn parse_lsda(lsda: &[u8]) -> gimli::Result<()> {
    let mut reader = EndianSlice::new(lsda, NativeEndian);

    // First, parse the header of the gcc LSDA table
    let lsda_header = LsdaHeader::parse(&mut reader)?;
    // Second, parse the call site table header
    let call_site_table_header = CallSiteTableHeader::parse(&mut reader)?;
    // Third, parse all of the call site table entries
    let end_of_call_site_table = reader.offset_id().0 + call_site_table_header.length;
    while reader.offset_id().0 < end_of_call_site_table {
        let entry = CallSiteTableEntry::parse(&mut reader, call_site_table_header.encoding)?;
        debug!("{:#X?}", entry);
        if entry.action_offset != 0 {
            warn!("unhandled call site action, offset (without 1 added): {:#X}", entry.action_offset);
        }
    }

    Ok(())
}



#[derive(Debug)]
struct LsdaHeader {
    /// The encoding used to read the next value `landing_pad_start`.
    landing_pad_start_encoding: DwEhPe,
    /// If the above `landing_pad_start_encoding` is not omitted,
    /// then this is the value that should be used as the base address of the landing pad,
    /// which is used by all the offsets specified in the LSDA call site tables and action tables.
    /// It is decoded using the above `landing_pad_start_encoding`, 
    /// which is typical cases is uleb128, but not always guaranteed to be.
    /// Otherwise, if omitted, the default value is the starting address range
    /// specified in the FDE (FrameDescriptionEntry) corresponding to this LSDA.
    landing_pad_start: Option<u64>,
    /// The encoding used to read pointer values in the type table.
    type_table_encoding: DwEhPe,
    /// If the above `type_table_encoding` is not omitted, 
    /// this is the offset to the type table, starting from the end of this header. 
    /// This is always encoded as a uleb128 value.
    /// If it was omitted above, then there is no type table,
    /// which is quite common in Rust-compiled object files.
    type_table_offset: Option<u64>,
}
impl LsdaHeader {
    fn parse<R: gimli::Reader>(reader: &mut R) -> gimli::Result<LsdaHeader> {
        let lp_encoding = DwEhPe(reader.read_u8()?);
        let lp = if lp_encoding == DW_EH_PE_omit {
            None
        } else {
            Some(read_encoded_pointer(reader, lp_encoding)?)
        };

        let tt_encoding = DwEhPe(reader.read_u8()?);
        let tt_offset = if tt_encoding == DW_EH_PE_omit {
            None
        } else {
            Some(read_encoded_pointer(reader, DW_EH_PE_uleb128)?)
        };

        Ok(LsdaHeader{
            landing_pad_start_encoding: lp_encoding,
            landing_pad_start: lp,
            type_table_encoding: tt_encoding,
            type_table_offset: tt_offset,
        })
    }
}




#[derive(Debug)]
struct CallSiteTableHeader {
    /// The encoding of items in the call site table.
    encoding: DwEhPe,
    /// The total length of the entire call site table, in bytes.
    /// This is always encoded in uleb128.
    length: u64,
}
impl CallSiteTableHeader {
    fn parse<R: gimli::Reader>(reader: &mut R) -> gimli::Result<CallSiteTableHeader> {
        let encoding = DwEhPe(reader.read_u8()?);
        let length = read_encoded_pointer(reader, DW_EH_PE_uleb128)?;
        Ok(CallSiteTableHeader {
            encoding, 
            length
        })
    }
}



#[derive(Debug)]
struct CallSiteTableEntry {
    start: u64,
    length: u64,
    landing_pad: u64,
    action_offset: u64,
}
impl CallSiteTableEntry {
    fn parse<R: gimli::Reader>(reader: &mut R, call_site_encoding: DwEhPe) -> gimli::Result<CallSiteTableEntry> {
        let cs_start  = read_encoded_pointer(reader, call_site_encoding)?;
        let cs_length = read_encoded_pointer(reader, call_site_encoding)?;
        let cs_lp     = read_encoded_pointer(reader, call_site_encoding)?;
        let cs_action = read_encoded_pointer(reader, DW_EH_PE_uleb128)?;
        Ok(CallSiteTableEntry {
            start: cs_start,
            length: cs_length,
            landing_pad: cs_lp,
            action_offset: cs_action,
        })
    }
}


fn read_encoded_pointer<R: gimli::Reader>(reader: &mut R, encoding: DwEhPe) -> gimli::Result<u64> {
    match encoding {
        DW_EH_PE_omit     => Err(gimli::Error::CannotParseOmitPointerEncoding),
        DW_EH_PE_absptr   => reader.read_u64().map(|v| v as u64),
        DW_EH_PE_uleb128  => reader.read_uleb128().map(|v| v as u64),
        DW_EH_PE_udata2   => reader.read_u16().map(|v| v as u64),
        DW_EH_PE_udata4   => reader.read_u32().map(|v| v as u64),
        DW_EH_PE_udata8   => reader.read_u64().map(|v| v as u64),
        DW_EH_PE_sleb128  => reader.read_sleb128().map(|v| v as u64),
        DW_EH_PE_sdata2   => reader.read_i16().map(|v| v as u64),
        DW_EH_PE_sdata4   => reader.read_i32().map(|v| v as u64),
        DW_EH_PE_sdata8   => reader.read_i64().map(|v| v as u64),
        _ => {
            error!("read_encoded_pointer(): unsupported pointer encoding: {:#X}: {:?}", 
                encoding.0,
                encoding.static_string()
            );
            Err(gimli::Error::UnknownPointerEncoding)
        }
    }
}
