//! Definitions for FADT, the Fixed ACPI Description Table.

#![no_std]

extern crate memory;
extern crate sdt;
extern crate acpi_table;
extern crate zerocopy;

use memory::PhysicalAddress;
use sdt::Sdt;
use acpi_table::{AcpiSignature, AcpiTables};
use zerocopy::FromBytes;


pub const FADT_SIGNATURE: &'static [u8; 4] = b"FACP";


/// The handler for parsing the FADT table and adding it to the ACPI tables list.
pub fn handle(
    acpi_tables: &mut AcpiTables,
    signature: AcpiSignature,
    _length: usize,
    phys_addr: PhysicalAddress
) -> Result<(), &'static str> {
    acpi_tables.add_table_location(signature, phys_addr, None)
}


#[repr(packed)]
#[derive(Clone, Copy, Debug, FromBytes)]
pub struct Fadt {
    pub header: Sdt,
    pub firmware_ctrl: u32,
    /// The physical address of the DSDT table
    pub dsdt: u32,
    _reserved: u8, 
    pub preferred_power_managament: u8,
    pub sci_interrupt: u16,
    pub smi_command_port: u32,
    pub acpi_enable: u8,
    pub acpi_disable: u8,
    pub s4_bios_req: u8,
    pub pstate_control: u8,
    pub pm1a_event_block: u32,
    pub pm1b_event_block: u32,
    pub pm1a_control_block: u32,
    pub pm1b_control_block: u32,
    pub pm2_control_block: u32,
    pub pm_timer_block: u32,
    pub gpe0_block: u32,
    pub gpe1_block: u32,
    pub pm1_event_length: u8,
    pub pm1_control_length: u8,
    pub pm2_control_length: u8,
    pub pm_timer_length: u8,
    pub gpe0_ength: u8,
    pub gpe1_length: u8,
    pub gpe1_base: u8,
    pub c_state_control: u8,
    pub worst_c2_latency: u16,
    pub worst_c3_latency: u16,
    pub flush_size: u16,
    pub flush_stride: u16,
    pub duty_offset: u8,
    pub duty_width: u8,
    pub day_alarm: u8,
    pub month_alarm: u8,
    pub century: u8,
    pub boot_architecture_flags: u16,
    _reserved2: u8,
    pub flags: u32,
}

impl Fadt {
    /// Finds the FADT in the given `AcpiTables` and returns a reference to it.
    pub fn get<'t>(acpi_tables: &'t AcpiTables) -> Option<&'t Fadt> {
        acpi_tables.table(&FADT_SIGNATURE).ok()
    }
}
