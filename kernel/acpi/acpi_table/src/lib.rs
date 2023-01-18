//! Definitions for the ACPI table
//!
//! RSDT is the Root System Descriptor Table, whereas
//! XSDT is the Extended System Descriptor Table. 
//! They are identical except that the XSDT uses 64-bit physical addresses
//! to point to other ACPI SDTs, while the RSDT uses 32-bit physical addresses.

#![no_std]

extern crate alloc;

use alloc::collections::BTreeMap;
use log::{trace, error};
use memory::{MappedPages, allocate_pages, allocate_frames_at, PageTable, PteFlags, PhysicalAddress, Frame, FrameRange};
use sdt::Sdt;
use core::ops::Add;
use zerocopy::FromBytes;

/// All ACPI tables are identified by a 4-byte signature,
/// typically an ASCII string like "APIC" or "RSDT".
pub type AcpiSignature = [u8; 4];

/// A record that tracks where an ACPI Table exists in memory,
/// given in terms of offsets into the `AcpiTables`'s `MappedPages`.
#[derive(Debug)]
pub struct TableLocation {
    /// The offset of the statically-sized part of the table,
    /// which is the entire table if there is no dynamically-sized component.
    pub offset: usize,
    /// The offset and length of the dynamically-sized part of the table, if it exists.
    /// If the entire table is statically-sized, this is `None`.
    pub slice_offset_and_length: Option<(usize, usize)>,
}

/// The struct holding all ACPI tables and records of where they exist in memory.
/// All ACPI tables are covered by a single large MappedPages object, 
/// which is necessary because they may span multiple pages/frames,
/// and generally should not be multiply aliased/accessed due to potential race conditions.
/// As more ACPI tables are discovered, the single MappedPages object is
/// extended to cover them.
pub struct AcpiTables {
    /// The range of pages that cover all of the discovered ACPI tables.
    mapped_pages: MappedPages,
    /// The physical memory frames that hold the ACPI tables,
    /// and are thus covered by the `mapped_pages`.
    frames: FrameRange,
    /// The location of all ACPI tables in memory.
    /// This is a mapping from ACPI table signature to the location in the `mapped_pages` object
    /// where the corresponding table is located.
    tables: BTreeMap<AcpiSignature, TableLocation>,
}

impl AcpiTables {
    /// Returns a new empty `AcpiTables` object.
    pub const fn empty() -> AcpiTables {
        AcpiTables {
            mapped_pages: MappedPages::empty(),
            frames: FrameRange::empty(),
            tables: BTreeMap::new(),
        }
    }

    /// Map the ACPI table that exists at the given PhysicalAddress, where an `SDT` header must exist.
    /// Ensures that the entire ACPI table is mapped, including extra length that may be specified within the SDT.
    /// 
    /// Returns a tuple describing the SDT discovered at the given `sdt_phys_addr`: 
    /// the `AcpiSignature` and the total length of the table.
    pub fn map_new_table(&mut self, sdt_phys_addr: PhysicalAddress, page_table: &mut PageTable) -> Result<(AcpiSignature, usize), &'static str> {

        // First, we map the SDT header so we can obtain its `length` field, 
        // which determines whether we need to map additional pages. 
        // Then, later, we'll obtain its `signature` field so we can invoke its specific handler 
        // that will add that table to the list of tables.
        let first_frame = Frame::containing_address(sdt_phys_addr);
        // If the Frame containing the given `sdt_phys_addr` wasn't already mapped, then we need to map it.
        if !self.frames.contains(&first_frame) {
            // Drop the current MappedPages and deallocate its frames so we can reallocate over them below. 
            let _orig_mp = core::mem::replace(&mut self.mapped_pages, MappedPages::empty());
            trace!("[0] Dropping original {:?}", _orig_mp);
            drop(_orig_mp);

            let new_frames = self.frames.to_extended(first_frame);
            let new_pages = allocate_pages(new_frames.size_in_frames())
                .ok_or("couldn't allocate pages for ACPI table")?;
            let af = allocate_frames_at(new_frames.start_address(), new_frames.size_in_frames())
                .map_err(|_e| "Couldn't allocate frames for ACPI table")?;
            let new_mapped_pages = page_table.map_allocated_pages_to(
                new_pages, 
                af,
                PteFlags::new().valid(true).writable(true),
            )?;

            self.adjust_mapping_offsets(new_frames, new_mapped_pages);
        }

        let sdt_offset = self.frames.offset_of_address(sdt_phys_addr)
            .ok_or("BUG: AcpiTables::map_new_table(): SDT physical address wasn't in expected frame iter")?;

        // Here we check if the header of the ACPI table fits at the offset.
        // If not, we add the next frame as well.
        if sdt_offset + core::mem::size_of::<Sdt>() > self.mapped_pages.size_in_bytes() {
            // Drop the current MappedPages and deallocate its frames so we can reallocate over them below. 
            let _orig_mp = core::mem::replace(&mut self.mapped_pages, MappedPages::empty());
            trace!("[1] Dropping original {:?}", _orig_mp);
            drop(_orig_mp);

            let new_frames = self.frames.to_extended(first_frame.add(1));
            let new_pages = allocate_pages(new_frames.size_in_frames())
                .ok_or("couldn't allocate pages for ACPI table")?;
            let af = allocate_frames_at(new_frames.start_address(), new_frames.size_in_frames())
                .map_err(|_e| "Couldn't allocate frames for ACPI table")?;
            let new_mapped_pages = page_table.map_allocated_pages_to(
                new_pages, 
                af,
                PteFlags::new().valid(true).writable(true),
            )?;

            self.adjust_mapping_offsets(new_frames, new_mapped_pages);
        }

        // Here, if the current mapped_pages is insufficient to cover the table's full length,
        // then we need to create a new mapping to cover it and the length of all of its entries.
        let (sdt_signature, sdt_length) = {
            let sdt: &Sdt = self.mapped_pages.as_type(sdt_offset)?;
            (sdt.signature, sdt.length as usize)
        };
        let last_frame_of_table = Frame::containing_address(sdt_phys_addr + sdt_length);
        if !self.frames.contains(&last_frame_of_table) {
            trace!("AcpiTables::map_new_table(): SDT's length requires mapping frames {:#X} to {:#X}", self.frames.end().start_address(), last_frame_of_table.start_address());
            // Drop the current MappedPages and deallocate its frames so we can reallocate over them below. 
            let _orig_mp = core::mem::replace(&mut self.mapped_pages, MappedPages::empty());
            trace!("[2] Dropping original {:?}", _orig_mp);
            drop(_orig_mp);

            let new_frames = self.frames.to_extended(last_frame_of_table);
            let new_pages = allocate_pages(new_frames.size_in_frames())
                .ok_or("couldn't allocate pages for ACPI table")?;
            let af = allocate_frames_at(new_frames.start_address(), new_frames.size_in_frames())
                .map_err(|_e| "Couldn't allocate frames for ACPI table")?;
            let new_mapped_pages = page_table.map_allocated_pages_to(
                new_pages, 
                af,
                PteFlags::new().valid(true).writable(true),
            )?;
            // No real need to adjust mapping offsets here, since we've only appended frames (not prepended);
            // we call this just to set the new frames and new mapped pages
            self.adjust_mapping_offsets(new_frames, new_mapped_pages);
        }

        // Here, the entire table is mapped into memory, and ready to be used elsewhere.
        Ok((sdt_signature, sdt_length))
    }

    /// Adjusts the offsets for all tables based on the new `MappedPages` and the new `FrameRange`.
    /// This object's (self) `frames` and `mappped_pages` will be replaced with the given items.
    fn adjust_mapping_offsets(&mut self, new_frames: FrameRange, new_mapped_pages: MappedPages) {
        // The basic idea here is that if we mapped new frames to the beginning of the mapped pages, 
        // then all of the table offsets will be wrong and need to be adjusted. 
        // To fix them, we simply add the number of bytes in the new frames that were prepended to the memory region.
        // For example, if two frames were added, then we need to add (2 * frame size) = 8192 to each offset.
        if new_frames.start() < self.frames.start() {
            let diff = self.frames.start_address().value() - new_frames.start_address().value();
            trace!("ACPI table: adjusting mapping offsets +{}", diff);
            for mut loc in self.tables.values_mut() {
                loc.offset += diff; 
                if let Some((ref mut slice_offset, _)) = loc.slice_offset_and_length {
                    *slice_offset += diff;
                }
            }
        }
        self.frames = new_frames;
        self.mapped_pages = new_mapped_pages;
    }

    /// Add the location and size details of a discovered ACPI table, 
    /// which allows others to query for and access the table in the future.
    /// 
    /// # Arguments
    /// * `signature`: the signature of the ACPI table that is being added, e.g., `b"RSDT"`.
    /// * `phys_addr`: the `PhysicalAddress` of the table in memory, which is used to calculate its offset.
    /// * `slice_phys_addr_and_length`: a tuple of the `PhysicalAddress` where the dynamic part of this table begins, 
    ///    and the number of elements in that dynamic table part.
    ///    If this table does not have a dynamic part, this is `None`.
    pub fn add_table_location(
        &mut self,
        signature: AcpiSignature,
        phys_addr: PhysicalAddress,
        slice_phys_addr_and_length: Option<(PhysicalAddress, usize)>
    ) -> Result<(), &'static str> {
        if self.table_location(&signature).is_some() {
            error!("AcpiTables::add_table_location(): signature {:?} already existed.", core::str::from_utf8(&signature));
            return Err("ACPI signature already existed");
        }

        let offset = self.frames.offset_of_address(phys_addr).ok_or("ACPI table's physical address is beyond the ACPI table bounds.")?;
        let slice_offset_and_length = if let Some((slice_paddr, slice_len)) = slice_phys_addr_and_length {
            Some((
                self.frames.offset_of_address(slice_paddr).ok_or("ACPI table's slice physical address is beyond the ACPI table bounds.")?,
                slice_len,
            ))
        } else { 
            None
        };

        self.tables.insert(signature, TableLocation { offset, slice_offset_and_length });
        Ok(())
    }

    /// Returns the location of the ACPI table based on the given table `signature`.
    pub fn table_location(&self, signature: &AcpiSignature) -> Option<&TableLocation> {
        self.tables.get(signature)
    }

    /// Returns a reference to the table that matches the specified ACPI `signature`.
    pub fn table<T: FromBytes>(&self, signature: &AcpiSignature) -> Result<&T, &'static str> {
        let loc = self.tables.get(signature).ok_or("couldn't find ACPI table with matching signature")?;
        self.mapped_pages.as_type(loc.offset)
    }

    /// Returns a mutable reference to the table that matches the specified ACPI `signature`.
    pub fn table_mut<T: FromBytes>(&mut self, signature: &AcpiSignature) -> Result<&mut T, &'static str> {
        let loc = self.tables.get(signature).ok_or("couldn't find ACPI table with matching signature")?;
        self.mapped_pages.as_type_mut(loc.offset)
    }

    /// Returns a reference to the dynamically-sized part at the end of the table that matches the specified ACPI `signature`,
    /// if it exists.
    /// For example, this returns the array of SDT physical addresses at the end of the [`RSDT`](../) table.
    pub fn table_slice<S: FromBytes>(&self, signature: &AcpiSignature) -> Result<&[S], &'static str> {
        let loc = self.tables.get(signature).ok_or("couldn't find ACPI table with matching signature")?;
        let (offset, len) = loc.slice_offset_and_length.ok_or("specified ACPI table has no dynamically-sized part")?;
        self.mapped_pages.as_slice(offset, len)
    }

    /// Returns a mutable reference to the dynamically-sized part at the end of the table that matches the specified ACPI `signature`,
    /// if it exists.
    /// For example, this returns the array of SDT physical addresses at the end of the [`RSDT`](../) table.
    pub fn table_slice_mut<S: FromBytes>(&mut self, signature: &AcpiSignature) -> Result<&mut [S], &'static str> {
        let loc = self.tables.get(signature).ok_or("couldn't find ACPI table with matching signature")?;
        let (offset, len) = loc.slice_offset_and_length.ok_or("specified ACPI table has no dynamically-sized part")?;
        self.mapped_pages.as_slice_mut(offset, len)
    }

    /// Returns an immutable reference to the underlying `MappedPages` that covers the ACPI tables.
    /// To access the ACPI tables, use the table's `get()` function, e.g., `Fadt::get(...)` instead of this function.
    pub fn mapping(&self) -> &MappedPages {
        &self.mapped_pages
    }
}
