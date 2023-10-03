//! Definitions for WAET, the Windows ACPI Emulated devices Table.

#![no_std]

use memory::PhysicalAddress;
use sdt::Sdt;
use acpi_table::{AcpiSignature, AcpiTables};
use zerocopy::FromBytes;


pub const WAET_SIGNATURE: &[u8; 4] = b"WAET";


/// The handler for parsing the WAET table and adding it to the ACPI tables list.
pub fn handle(
    acpi_tables: &mut AcpiTables,
    signature: AcpiSignature,
    _length: usize,
    phys_addr: PhysicalAddress
) -> Result<(), &'static str> {
    acpi_tables.add_table_location(signature, phys_addr, None)
}


/// The Windows ACPI Emulated devices Table (WAET) allows virtualized OSes
/// to avoid workarounds for errata on physical devices.
///
/// <https://download.microsoft.com/download/7/E/7/7E7662CF-CBEA-470B-A97E-CE7CE0D98DC2/WAET.docx>
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, FromBytes)]
pub struct Waet {
    pub header: Sdt,
    pub emulated_device_flags: u32,
}
const _: () = assert!(core::mem::size_of::<Waet>() == 40);
const _: () = assert!(core::mem::align_of::<Waet>() == 1);

impl Waet {
    /// Finds the WAET in the given `AcpiTables` and returns a reference to it.
    pub fn get(acpi_tables: &AcpiTables) -> Option<&Waet> {
        acpi_tables.table(WAET_SIGNATURE).ok()
    }

    /// Returns whether the RTC has been enhanced not to require
    /// acknowledgment after it asserts an interrupt.
    ///
    /// If this returns `true`, an interrupt handler can bypass
    /// reading the RTC register to unlatch the pending interrupt.
    pub fn rtc_good(&self) -> bool {
        const RTC_GOOD: u32 = 1 << 0;
        self.emulated_device_flags & RTC_GOOD == RTC_GOOD
    }

    /// Returns whether the ACPI PM timer has been enhanced not to require
    /// multiple reads.
    ///
    /// If this returns `true`, only a single read of the ACPI PM timer is
    /// necessary to obtain a reliable value from it.
    pub fn acpi_pm_timer_good(&self) -> bool {
        const ACPI_PM_TIMER_GOOD: u32 = 1 << 1;
        self.emulated_device_flags & ACPI_PM_TIMER_GOOD == ACPI_PM_TIMER_GOOD
    }
}
