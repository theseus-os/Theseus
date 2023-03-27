//! Support for the DRHD: DMAR Hardware Unit Definition Structure.

use super::*;

/// DRHD: DMAR Hardware Unit Definition Structure.
///
/// This table is described in Section 8.3 of the VT Directed I/O Spec.
#[derive(Clone, Copy, Debug, FromBytes)]
#[repr(packed)]
pub(crate) struct Drhd {
    _header: DmarEntryRecord,
    flags: u8,
    _reserved: u8,
    segment_number: u16,
    register_base_address: u64,
    // Following this is a variable number of variable-sized DMAR device scope table entries,
    // so we cannot include them here in the static struct definition.
}
const _: () = assert!(core::mem::size_of::<Drhd>() == 16);
const _: () = assert!(core::mem::align_of::<Drhd>() == 1);


/// DRHD: DMAR Hardware Unit Definition Structure.
///
/// This table is described in Section 8.3 of the VT Directed I/O Spec.
#[derive(Debug)]
pub struct DmarDrhd<'t> {
    /// The fixed-size part of the actual DRHD ACPI table.
    table: &'t Drhd,
    /// The underlying MappedPages that cover this table.
    mapped_pages: &'t MappedPages,
    /// The offset into the above `mapped_pages` at which the dynamic part 
    /// (the [`DmarDeviceScope`] structures) of the DRHD table begins.
    dynamic_entries_starting_offset: usize,
    /// The total size in bytes of all dynamic [`DmarDeviceScope`] entries.
    /// This is *not* the number of entries.
    dynamic_entries_total_size: usize,
}

impl<'t> DmarDrhd<'t> {
    pub(crate) fn from_entry(
        mp: &'t MappedPages,
        mp_offset: usize,
        entry: &DmarEntryRecord,
    ) -> Result<DmarDrhd<'t>, &'static str> {
        Ok(DmarDrhd {
            table: mp.as_type(mp_offset)?,
            mapped_pages: mp,
            dynamic_entries_starting_offset: mp_offset + size_of::<Drhd>(), 
            dynamic_entries_total_size: entry.length as usize - size_of::<Drhd>(),
        })
    }
}


impl<'t> DmarDrhd<'t> {
    /// Returns an [`Iterator`] over the [`DmarDeviceScope`] entries in this DRHD,
    /// which are variable in both number and size.
    pub fn iter(&self) -> DrhdIter<'t> {
        DrhdIter {
            mapped_pages: self.mapped_pages,
            offset: self.dynamic_entries_starting_offset,
            end_of_entries: self.dynamic_entries_starting_offset + self.dynamic_entries_total_size,
        }
    }

    /// Returns the value of the `INCLUDE_PCI_ALL` flag,
    /// the only bit flag in this DRHD table.
    ///
    /// # Description from Intel Spec
    /// If `false`, this remapping hardware unit has under its scope only
    /// devices in the specified segment that are explicitly identified through
    /// the Device Scope field. The device can be of any type as described by
    /// the Type field in the Device Scope Structure including (but not limited to)
    /// IOAPIC and HPET.
    /// 
    /// If `true`, this remapping hardware unit has under its scope all PCI
    /// compatible devices in the specified segment, except devices reported
    /// under the scope of other remapping hardware units for the same segment. 
    /// As such, one can use the Device Scope structures to enumerate 
    /// IOAPIC and HPET devices under its scope.
    pub fn include_pci_all(&self) -> bool {
        self.table.flags & 0x01 == 0x01
    }

    /// Returns the PCI segment number associated with this DRHD.
    pub fn segment_number(&self) -> u16 {
        self.table.segment_number
    }

    /// Returns the base address of this DRHD's remapping hardware register set.
    pub fn register_base_address(&self) -> u64 {
        self.table.register_base_address
    }
}


/// An [`Iterator`] over the dynamic entries ([`DmarDeviceScope`]s) of the [`DmarDrhd`].
/// Its lifetime is dependent upon the lifetime of its [`DmarDrhd`] instance,
/// which itself is bound to the lifetime of the underlying [`AcpiTables`]. 
#[derive(Clone)]
pub struct DrhdIter<'t> {
    /// The underlying MappedPages that contain all ACPI tables.
    mapped_pages: &'t MappedPages,
    /// The offset of the next entry, which should point to a [`DmarDeviceScope`]
    /// at the start of each iteration.
    offset: usize,
    /// The end bound of all DRHD entries. 
    /// This is fixed and should not ever change throughout iteration.
    end_of_entries: usize,
}

impl<'t> Iterator for DrhdIter<'t> {
    type Item = DmarDeviceScope<'t>;

    fn next(&mut self) -> Option<Self::Item> {
        if (self.offset + size_of::<DeviceScope>()) < self.end_of_entries {
            if let Ok(dev_scope) = DmarDeviceScope::from_entry(self.mapped_pages, self.offset) {
                self.offset += dev_scope.length() as usize;
                return Some(dev_scope);
            }
        }
        None
    }
}