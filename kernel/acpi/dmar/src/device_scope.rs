//! Support for the DMAR Device Scope Structure.

use super::*;

/// DMAR Device Scope Structure.
///
/// This structure is described in Section 8.3.1 of the VT Directed I/O Spec.
#[derive(Clone, Copy, Debug, FromBytes)]
#[repr(packed)]
pub(crate) struct DeviceScope {
    typ: u8,
    length: u8,
    _reserved: u16,
    enumeration_id: u8,
    start_bus_number: u8,
    // Following this is a variable-sized `Path` field,
    // so we cannot include it here in the static struct definition.
    // It would look something like:
    // path: [u16],
}
const _: () = assert!(core::mem::size_of::<DeviceScope>() == 6);
const _: () = assert!(core::mem::align_of::<DeviceScope>() == 1);


/// DMAR Device Scope Structure.
///
/// This structure is described in Section 8.3.1 of the VT Directed I/O Spec.
#[derive(Debug)]
pub struct DmarDeviceScope<'t> {
    /// The fixed-size part of the [`DeviceScope`] structure.
    table: &'t DeviceScope,
    /// The underlying MappedPages that cover this structure.
    mapped_pages: &'t MappedPages,
    /// The offset into the above `mapped_pages` at which 
    /// the dynamic part (the Path) of the [`DeviceScope`] structure begins.
    path_starting_offset: usize,
    /// The total size in bytes of all dynamic Path entries.
    /// The number of Path entries is `path_total_size / 2`.
    #[allow(dead_code)]
    path_total_size: usize,
}

impl<'t> DmarDeviceScope<'t> {
    pub(crate) fn from_entry(
        mp: &'t MappedPages,
        mp_offset: usize,
    ) -> Result<DmarDeviceScope<'t>, &'static str> {
        let dev_scope: &DeviceScope = mp.as_type(mp_offset)?;
        Ok(DmarDeviceScope {
            table: dev_scope,
            mapped_pages: mp,
            path_starting_offset: mp_offset + size_of::<DeviceScope>(), 
            path_total_size: dev_scope.length as usize - size_of::<DeviceScope>(),
        })
    }

    /// Returns the type of this device scope structure.
    /// 
    /// TODO: use an enum to represent possible device types.
    ///
    /// * `1`: PCI Endpoint Device - The device identified by the ‘Path’ field is
    ///   a PCI endpoint device. This type must not be used in Device Scope of
    ///   DRHD structures with INCLUDE_PCI_ALL flag Set.
    /// * `2`: PCI Sub-hierarchy - The device identified by the ‘Path’ field is a
    ///   PCI-PCI bridge. In this case, the specified bridge device and all its
    ///   downstream devices are included in the scope. This type must not be
    ///   in Device Scope of DRHD structures with INCLUDE_PCI_ALL flag Set.
    /// * `3`: IOAPIC - The device identified by the ‘Path’ field is an I/O APIC
    ///   (or I/O SAPIC) device, enumerated through the ACPI MADT I/O APIC
    ///   (or I/O SAPIC) structure.
    /// * `4`: MSI_CAPABLE_HPET1 - The device identified by the ‘Path’ field
    ///   is an HPET Timer Block capable of generating MSI (Message Signaled
    ///   interrupts). HPET hardware is reported through ACPI HPET structure.
    /// * `5`: ACPI_NAMESPACE_DEVICE - The device identified by the ‘Path’
    ///   field is an ACPI name-space enumerated
    pub fn device_type(&self) -> u8 {
        self.table.typ
    }

    pub(crate) fn length(&self) -> u8 { self.table.length }

    /// Returns the Enumeration ID, which differs in meaning based on the type
    /// of this [`DmarDeviceScope`] structure.
    pub fn enumeration_id(&self) -> u8 {
        self.table.enumeration_id
    }

    /// Returns the PCI bus number under which the device identified
    /// by this [`DmarDeviceScope`] exists.
    pub fn start_bus_number(&self) -> u8 {
        self.table.start_bus_number
    }

    /// Calculates and returns the hierarchical path (along the PCI bus)
    /// to the device specified by this [`DmarDeviceScope`] structure.
    /// 
    /// # Warning -- incomplete!
    /// TODO: finish this function, it is not yet complete. It only returns the first path. 
    pub fn path(&self) -> Result<&'t DeviceScopePath, &'static str> {
        log::warn!("The DmarDeviceScope::path() function is incomplete! Its value may be incorrect.");
        let /* mut */ offset = self.path_starting_offset;
        let starting_path: &DeviceScopePath = self.mapped_pages.as_type(offset)?;

        // TODO: complete the path iteration algorithm described in Section 8.3.1
        Ok(starting_path)
    }   
}

#[derive(Debug, Clone, Copy, FromBytes)]
#[repr(packed)]
pub struct DeviceScopePath {
    pub device: u8,
    pub function: u8,
}
const _: () = assert!(core::mem::size_of::<DeviceScopePath>() == 2);
const _: () = assert!(core::mem::align_of::<DeviceScopePath>() == 1);
