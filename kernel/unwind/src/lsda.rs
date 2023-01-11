//! Routines for parsing the gcc-style LSDA (Language-Specific Data Area) in an ELF object file, 
//! which corresponds to an area within a `.gcc_except_table` section. 

#![allow(nonstandard_style)]

use core::ops::Range;
use gimli::{Reader, DwEhPe, Endianity, EndianSlice, constants::*, };
use FallibleIterator;

/// `GccExceptTableArea` contains the contents of the Language-Specific Data Area (LSDA)
/// that is used to locate cleanup (run destructors for) a given function during stack unwinding. 
/// 
/// Though an object file typically only includes a single `.gcc_except_table` section, it may include multiple.
/// This struct represents only *one area* of that section, the area that is for cleaning up a single function. 
/// There may exist multiple instances of this struct created as overlays 
/// for different, non-overlapping areas of that one `.gcc_except_table` section.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GccExceptTableArea<R: Reader> {
    reader: R,
    function_start_address: u64,
}

impl<'input, Endian: Endianity> GccExceptTableArea<EndianSlice<'input, Endian>> {
    /// Construct a new `GccExceptTableArea` instance from the given input data,
    /// which is a slice that typically begins at an LSDA pointer that was parsed
    /// from a `FrameDescriptionEntry` in the `EhFrame` section.
    /// 
    /// The starting address of the function that it corresponds to must aso be provided,
    /// because this is often used as the default base address for the landing pad
    /// from which all offsets are calculated.
    pub fn new(data: &'input [u8], endian: Endian, function_start_address: u64) -> Self {
        GccExceptTableArea {
            reader: EndianSlice::new(data, endian),
            function_start_address,
        }
    }
}

// impl<R: Reader> Section<R> for GccExceptTableArea<R> {
//     // TODO FIXME: this is wrong (temporarily), 
//     // once we incoporate it into Gimli itself, we can add a GccExceptFrame sectionid.
//     fn id() -> SectionId { SectionId::EhFrameHdr }

//     fn reader(&self) -> &R { &self.reader }

//     fn section_name() -> &'static str {
//         ".gcc_except_table"
//     }
// }

impl<R: Reader> GccExceptTableArea<R> {
    /// Parses the .gcc_except_table entries from the very top of the LSDA area.
    /// This only parses the two headers that are guaranteed to exist,
    /// the other dynamically-sized entries should be parsed elsewhere using the result of this function.
    /// 
    /// The flow of this code was partially inspired by rust's stdlib `libpanic_unwind/dwarf/eh.rs` file.
    /// <https://github.com/rust-lang/rust/blob/master/src/libpanic_unwind/dwarf/eh.rs>
    fn parse_from_beginning(&self) -> gimli::Result<(LsdaHeader, CallSiteTableHeader, R)> {
        // Clone the internal `Reader` to avoid modifying the offset position of the original provided reader.
        let mut reader = self.reader.clone();

        // First, parse the top-level header, which comes at the very beginning
        let lsda_header = LsdaHeader::parse(&mut reader)?;
        // debug!("{:#X?}", lsda_header);

        // Second, parse the call site table header, which comes right after the top-level LSDA header
        let call_site_table_header = CallSiteTableHeader::parse(&mut reader)?;
        // debug!("{:#X?}", call_site_table_header);

        Ok((lsda_header, call_site_table_header, reader))
    }

    /// Returns an iterator over all of the call site entries
    /// found in this area of this .gcc_except_table section.
    /// 
    /// Can be used with the `FallibleIterator` trait. 
    pub fn call_site_table_entries(&self) -> gimli::Result<CallSiteTableIterator<R>> {
        let (lsda_header, call_site_table_header, reader) = self.parse_from_beginning()?;
        // set up the call site table iterator so it knows when to stop parsing entries.        
        let end_of_call_site_table = reader.offset_id().0 + call_site_table_header.length;

        Ok(CallSiteTableIterator {
            call_site_table_encoding: call_site_table_header.encoding,
            end_of_call_site_table,
            landing_pad_base: lsda_header.landing_pad_base.unwrap_or(self.function_start_address),
            reader, 
        })
    }

    /// Iterates over the call site table and finds the entry that matches the given instruction pointer (IP), 
    /// i.e., the entry that covers the range of addresses that the `ip` falls within.
    pub fn call_site_table_entry_for_address(&self, address: u64) -> gimli::Result<CallSiteTableEntry> {
        let mut iter = self.call_site_table_entries()?;
        while let Some(entry) = iter.next()? {
            if entry.range_of_covered_addresses().contains(&address) {
                return Ok(entry);
            }
        }
        Err(gimli::Error::NoUnwindInfoForAddress)
    }
}

/// The header of an LSDA section, which is at the very beginning of the area 
/// in the .gcc_except_table section that was pointed to by a stack frame's LSDA pointer. 
#[derive(Debug)]
#[allow(dead_code)]
struct LsdaHeader {
    /// The encoding used to read the next value `landing_pad_base`.
    landing_pad_base_encoding: DwEhPe,
    /// If the above `landing_pad_base_encoding` is not `DW_EH_PE_omit`,
    /// then this is the value that should be used as the base address of the landing pad,
    /// which is used by all the offsets specified in the LSDA call site tables and action tables.
    /// It is decoded using the above `landing_pad_base_encoding`, 
    /// which is typically the uleb128 encoding, but not always guaranteed to be.
    /// Otherwise, if `DW_EH_PE_omit`, the default value is the starting function address
    /// specified in the FDE (FrameDescriptionEntry) corresponding to this LSDA.
    /// 
    /// Typically, this will be the virtual address of the function that this cleanup routine is for;
    /// such cleanup routines are usually at the end of the function's text section.
    landing_pad_base: Option<u64>,
    /// The encoding used to read pointer values in the type table.
    type_table_encoding: DwEhPe,
    /// If the above `type_table_encoding` is not `DW_EH_PE_omit`, 
    /// this is the offset to the type table, starting from the end of this header. 
    /// This is always encoded as a uleb128 value.
    /// If it was `DW_EH_PE_omit` above, then there is no type table,
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
            landing_pad_base_encoding: lp_encoding,
            landing_pad_base: lp,
            type_table_encoding: tt_encoding,
            type_table_offset: tt_offset,
        })
    }
}


/// The header of the call site table, which defines the length of the table 
/// and the encoding format used to parse address values in the table.
/// The call site table comes immediately after the `LsdaHeader`.
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


/// An entry in the call site table, which defines landing pad functions and additional actions
/// that should be executed when unwinding a given a stack frame. 
/// The relevant entry for a particular stack frame can be found based on the range of addresses it covers.
#[derive(Debug)]
pub struct CallSiteTableEntry {
    /// An offset from the landing pad base address (top of function section)
    /// that specifies the first (starting) address that is covered by this entry.
    starting_offset: u64,
    /// The length (from the above `starting_offset`) that specifies
    /// the range of addresses covered by this entry.
    length: u64,
    /// The offset from the `landing_pad_base` at which the landing pad entry function exists.
    /// If `0`, then there is no landing pad function that should be run.
    landing_pad_offset: u64,
    /// The offset into the action table that specifies what additional action to undertake.
    /// If `0`, there is no action;
    /// otherwise, this value plus 1 (`action_offset + 1`) should be used to locate the action table entry.
    action_offset: u64,

    /// The starting address of the function that this GccExceptTableArea pertains to.
    /// This is not actually part of the table entry as defined in the gcc LSDA spec,
    /// it comes from the top-level LSDA header and is replicated here for convenience. 
    landing_pad_base: u64
}
impl CallSiteTableEntry {
    fn parse<R: gimli::Reader>(
        reader: &mut R,
        call_site_encoding: DwEhPe,
        landing_pad_base: u64,
    ) -> gimli::Result<CallSiteTableEntry> {
        let cs_start  = read_encoded_pointer(reader, call_site_encoding)?;
        let cs_length = read_encoded_pointer(reader, call_site_encoding)?;
        let cs_lp     = read_encoded_pointer(reader, call_site_encoding)?;
        let cs_action = read_encoded_pointer(reader, DW_EH_PE_uleb128)?;
        Ok(CallSiteTableEntry {
            starting_offset: cs_start,
            length: cs_length,
            landing_pad_offset: cs_lp,
            action_offset: cs_action,
            landing_pad_base,
        })
    }

    /// The range of addresses (instruction pointers) that are covered by this entry. 
    pub fn range_of_covered_addresses(&self) -> Range<u64> {
        (self.landing_pad_base + self.starting_offset) .. (self.landing_pad_base + self.starting_offset + self.length)
    }

    /// The address of the actual landing pad, i.e., the cleanup routine that should run, if one exists.
    pub fn landing_pad_address(&self) -> Option<u64> {
        if self.landing_pad_offset == 0 { 
            None
        } else {
            Some(self.landing_pad_base + self.landing_pad_offset)
        }
    }

    /// The offset into the action table that specifies which additional action should be undertaken
    /// when invoking this landing pad cleanup routine, if one exists. 
    pub fn action_offset(&self) -> Option<u64> {
        if self.action_offset == 0 { 
            // no action to perform
            None
        } else {
            // the gcc_except_table docs specify that 1 must be added to the action offset if it is not zero.
            Some(self.action_offset + 1)
        }
    }
}

/// An iterator over all of the entries in a GccExceptTableArea's call site table.
/// 
/// Can be used with the `FallibleIterator` trait.
pub struct CallSiteTableIterator<R: Reader> {
    /// The encoding of pointers in the call site table.
    call_site_table_encoding: DwEhPe,
    /// This is the ending bound for the following `reader` to parse,
    /// i.e., the reader offset right after the final call site table entry.
    end_of_call_site_table: u64,
    /// The starting address of the function that this GccExceptTableArea pertains to.
    landing_pad_base: u64,
    /// This reader must be queued up to the beginning of the first call site table entry,
    /// i.e., right after the end of the call site table header.
    reader: R,
}

impl<R: Reader> FallibleIterator for CallSiteTableIterator<R> {
    type Item = CallSiteTableEntry;
    type Error = gimli::Error;

    fn next(&mut self) -> Result<Option<Self::Item>, Self::Error> {
        if self.reader.offset_id().0 < self.end_of_call_site_table {
            let entry = CallSiteTableEntry::parse(&mut self.reader, self.call_site_table_encoding, self.landing_pad_base)?;
            if let Some(action_offset) = entry.action_offset() {
                #[cfg(not(downtime_eval))]
                warn!("unsupported/unhandled call site action, offset (with 1 added): {:#X}", action_offset);
            }
            Ok(Some(entry))
        } else {
            Ok(None)
        }
    }
}

/// Decodes the next pointer from the given `reader` (a stream of bytes) using the given `encoding` format. 
fn read_encoded_pointer<R: gimli::Reader>(reader: &mut R, encoding: DwEhPe) -> gimli::Result<u64> {
    match encoding {
        DW_EH_PE_omit     => Err(gimli::Error::CannotParseOmitPointerEncoding),
        DW_EH_PE_absptr   => reader.read_u64(),
        DW_EH_PE_uleb128  => reader.read_uleb128(),
        DW_EH_PE_udata2   => reader.read_u16().map(|v| v as u64),
        DW_EH_PE_udata4   => reader.read_u32().map(|v| v as u64),
        DW_EH_PE_udata8   => reader.read_u64(),
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
