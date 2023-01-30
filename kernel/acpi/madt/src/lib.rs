//! Support for the MADT ACPI table, 
//! which includes interrupt and multicore info.

#![no_std]

extern crate alloc;

use core::mem::size_of;
use alloc::{boxed::Box, format};
use log::{error, warn, trace};
use memory::{MappedPages, PageTable, PhysicalAddress}; 
use apic::{LocalApic, bootstrap_cpu, LapicInitError, current_cpu};
use sdt::Sdt;
use acpi_table::{AcpiSignature, AcpiTables};
use zerocopy::FromBytes;

pub const MADT_SIGNATURE: &[u8; 4] = b"APIC";

/// The handler for parsing the MADT table and adding it to the ACPI tables list.
pub fn handle(
    acpi_tables: &mut AcpiTables,
    signature: AcpiSignature,
    _length: usize,
    phys_addr: PhysicalAddress
) -> Result<(), &'static str> {
    // The MADT has a variable number of entries, and each entry is of variable size. 
    // So we can't determine the slice_length (just use 0 instead), but we can determine where it starts.
    let slice_start_paddr = phys_addr + size_of::<MadtAcpiTable>();
    acpi_tables.add_table_location(signature, phys_addr, Some((slice_start_paddr, 0)))
}


/// The fixed-size components of the MADT ACPI table (Multiple APIC Descriptor Table).
/// Its layout and total size must exactly match that of the ACPI specification.
/// 
/// Note that this is only the fixed-size part of the MADT table.
/// At the end, there is an unknown number of table entries, each of variable size. 
/// Thus, we cannot pre-define them here, but only discover/define them in the iterator.
#[derive(Clone, Copy, Debug, FromBytes)]
#[repr(C, packed)]
struct MadtAcpiTable {
    header: Sdt,
    local_apic_phys_addr: u32,
    flags: u32,
    // Following this is a variable number of variable-sized table entries,
    // so we cannot include them here.
}
const _: () = assert!(core::mem::size_of::<MadtAcpiTable>() == 44);
const _: () = assert!(core::mem::align_of::<MadtAcpiTable>() == 1);


/// A wrapper around the MADT ACPI table (Multiple APIC Descriptor Table),
/// which contains details about multicore and interrupt configuration.
/// 
/// You most likely only care about the `iter()` method,
/// though other fields of the MADT are accessible.
pub struct Madt<'t> {
    /// The fixed-size part of the actual MADT ACPI table.
    table: &'t MadtAcpiTable,
    /// The underlying MappedPages that cover this MADT
    mapped_pages: &'t MappedPages,
    /// The offset into the above `mapped_pages` at which the dynamic part
    /// of the MADT table begins.
    dynamic_entries_starting_offset: usize,
    /// The total size in bytes of all dynamic entries.
    /// This is *not* the number of entries.
    dynamic_entries_total_size: usize,
}

impl<'t> Madt<'t> {
    /// Finds the MADT in the given `AcpiTables` and returns a reference to it.
    pub fn get(acpi_tables: &'t AcpiTables) -> Option<Madt<'t>> {
        let table: &MadtAcpiTable = acpi_tables.table(MADT_SIGNATURE).ok()?;
        let total_length = table.header.length as usize;
        let dynamic_part_length = total_length - size_of::<MadtAcpiTable>();
        let loc = acpi_tables.table_location(MADT_SIGNATURE)?;
        Some(Madt {
            table,
            mapped_pages: acpi_tables.mapping(),
            dynamic_entries_starting_offset: loc.slice_offset_and_length?.0,
            dynamic_entries_total_size: dynamic_part_length,
        })
    }

    /// Performs initialization functions of the IOAPIC and bootstrap processor.
    /// # Important Note
    /// This should only be called once from the initial bootstrap processor 
    /// (the first core to run).
    pub fn bsp_init(&self, page_table: &mut PageTable) -> Result<(), &'static str> {
        handle_ioapic_entries(self.iter(), page_table)?;
        handle_bsp_lapic_entry(self.iter(), page_table)?;
        Ok(())
    }

    /// Returns an [`Iterator`] over the MADT's entries,
    /// which are variable in both number and size.
    pub fn iter(&self) -> MadtIter {
        MadtIter {
            mapped_pages: self.mapped_pages,
            offset: self.dynamic_entries_starting_offset,
            end_of_entries: self.dynamic_entries_starting_offset + self.dynamic_entries_total_size,
        }
    }

    /// Returns a reference to the `Sdt` header in this MADT table.
    pub fn sdt(&self) -> &Sdt {
        &self.table.header
    }

    /// Returns the Local APIC physical address value in this MADT table.
    pub fn local_apic_phys_addr(&self) -> u32 {
        self.table.local_apic_phys_addr
    }

    /// Returns the `flags` value in this MADT table.
    pub fn flags(&self) -> u32 {
        self.table.flags
    }
}


/// An [`Iterator`] over the dynamic entries of the MADT.
/// Its lifetime is dependent upon the lifetime of its `Madt` instance,
/// which itself is bound to the lifetime of the underlying `AcpiTables`. 
#[derive(Clone)]
pub struct MadtIter<'t> {
    /// The underlying MappedPages that contain all ACPI tables.
    mapped_pages: &'t MappedPages,
    /// The offset of the next entry, which should point to a `EntryRecord`
    /// at the start of each iteration.
    offset: usize,
    /// The end bound of all MADT entries. 
    /// This is fixed and should not ever change throughout iteration.
    end_of_entries: usize,
}

impl<'t> Iterator for MadtIter<'t> {
    type Item = MadtEntry<'t>;

    fn next(&mut self) -> Option<Self::Item> {
        if (self.offset + ENTRY_RECORD_SIZE) < self.end_of_entries {
            // First, we get the next entry record to get the type and size of the actual entry.
            let (entry_type, entry_size) = { 
                let entry_record: &EntryRecord = self.mapped_pages.as_type(self.offset).ok()?;
                (entry_record.typ, entry_record.size as usize)
            };
            // Second, use that entry type and size to return the specific Madt entry struct.
            if (self.offset + entry_size) <= self.end_of_entries {
                let entry: Option<MadtEntry> = match entry_type {
                    ENTRY_TYPE_LOCAL_APIC if entry_size == size_of::<MadtLocalApic>() => {
                        self.mapped_pages.as_type(self.offset).ok().map(MadtEntry::LocalApic)
                    },
                    ENTRY_TYPE_IO_APIC if entry_size == size_of::<MadtIoApic>() => {
                        self.mapped_pages.as_type(self.offset).ok().map(MadtEntry::IoApic)
                    },
                    ENTRY_TYPE_INT_SRC_OVERRIDE if entry_size == size_of::<MadtIntSrcOverride>() => {
                        self.mapped_pages.as_type(self.offset).ok().map(MadtEntry::IntSrcOverride)
                    },
                    ENTRY_TYPE_NON_MASKABLE_INTERRUPT if entry_size == size_of::<MadtNonMaskableInterrupt>() => {
                        self.mapped_pages.as_type(self.offset).ok().map(MadtEntry::NonMaskableInterrupt)
                    },
                    ENTRY_TYPE_LOCAL_APIC_ADDRESS_OVERRIDE if entry_size == size_of::<MadtLocalApicAddressOverride>() => {
                        self.mapped_pages.as_type(self.offset).ok().map(MadtEntry::LocalApicAddressOverride)
                    },
                    _ => None,
                };
                // move the offset to the end of this entry, i.e., the beginning of the next entry record
                self.offset += entry_size;
                // return the MADT entry if properly formed, or if not, return an unknown/corrupt entry.
                entry.or(Some(MadtEntry::UnknownOrCorrupt(entry_type)))
            }
            else {
                None
            }
        }
        else {
            None
        }
    }
}


/// A MADT entry record, which precedes each actual MADT entry
/// and describes its type and size.
#[derive(Clone, Copy, Debug, FromBytes)]
#[repr(packed)]
struct EntryRecord {
    /// The type identifier of a MADT entry.
    typ: u8,
    /// The size in bytes of a MADT entry.
    size: u8,
}
const ENTRY_RECORD_SIZE: usize = size_of::<EntryRecord>();
const _: () = assert!(core::mem::size_of::<EntryRecord>() == 2);
const _: () = assert!(core::mem::align_of::<EntryRecord>() == 1);


// The following list specifies MADT entry type IDs.
const ENTRY_TYPE_LOCAL_APIC:                  u8 = 0;
const ENTRY_TYPE_IO_APIC:                     u8 = 1;
const ENTRY_TYPE_INT_SRC_OVERRIDE:            u8 = 2;
// entry type 3 doesn't exist
const ENTRY_TYPE_NON_MASKABLE_INTERRUPT:      u8 = 4;
const ENTRY_TYPE_LOCAL_APIC_ADDRESS_OVERRIDE: u8 = 5;


/// The set of possible MADT Entries.
#[derive(Copy, Clone, Debug)]
pub enum MadtEntry<'t> {
    /// A Local APIC MADT entry.
    LocalApic(&'t MadtLocalApic),
    /// A IOAPIC MADT entry.
    IoApic(&'t MadtIoApic),
    /// A Interrupt Source Override MADT entry.
    IntSrcOverride(&'t MadtIntSrcOverride),
    /// A Non-Maskable Interrupt MADT entry.
    NonMaskableInterrupt(&'t MadtNonMaskableInterrupt),
    /// A Local APIC Address Override MADT entry.
    LocalApicAddressOverride(&'t MadtLocalApicAddressOverride),
    /// The MADT table had an entry of an unknown type or mismatched length,
    /// so the table entry was malformed and unusable.
    /// The entry type ID is included.
    UnknownOrCorrupt(u8)
}

/// MADT Local APIC
#[derive(Copy, Clone, Debug, FromBytes)]
#[repr(packed)]
pub struct MadtLocalApic {
    _header: EntryRecord,
    /// Processor ID
    pub processor: u8,
    /// Local APIC ID
    pub apic_id: u8,
    /// Flags. 1 means that the processor is enabled
    pub flags: u32
}
const _: () = assert!(core::mem::size_of::<MadtLocalApic>() == 8);
const _: () = assert!(core::mem::align_of::<MadtLocalApic>() == 1);

/// MADT I/O APIC
#[derive(Copy, Clone, Debug, FromBytes)]
#[repr(packed)]
pub struct MadtIoApic {
    _header: EntryRecord,
    /// I/O APIC ID
    pub id: u8,
    _reserved: u8,
    /// I/O APIC address
    pub address: u32,
    /// Global system interrupt base
    pub gsi_base: u32
}
const _: () = assert!(core::mem::size_of::<MadtIoApic>() == 12);
const _: () = assert!(core::mem::align_of::<MadtIoApic>() == 1);

/// MADT Interrupt Source Override
#[derive(Copy, Clone, Debug, FromBytes)]
#[repr(packed)]
pub struct MadtIntSrcOverride {
    _header: EntryRecord,
    /// Bus Source
    pub bus_source: u8,
    /// IRQ Source
    pub irq_source: u8,
    /// Global system interrupt
    pub gsi: u32,
    /// Flags
    pub flags: u16
}
const _: () = assert!(core::mem::size_of::<MadtIntSrcOverride>() == 10);
const _: () = assert!(core::mem::align_of::<MadtIntSrcOverride>() == 1);

/// MADT Non-maskable Interrupt.
/// Use these to configure the LINT0 and LINT1 entries in the Local vector table
/// of the relevant processor's (or processors') local APIC.
#[derive(Copy, Clone, Debug, FromBytes)]
#[repr(packed)]
pub struct MadtNonMaskableInterrupt {
    _header: EntryRecord,
    /// which processor this is for, 0xFF means all processors
    pub processor: u8,
    /// Flags
    pub flags: u16,
    /// LINT (either 0 or 1)
    pub lint: u8,
}
const _: () = assert!(core::mem::size_of::<MadtNonMaskableInterrupt>() == 6);
const _: () = assert!(core::mem::align_of::<MadtNonMaskableInterrupt>() == 1);

/// MADT Local APIC Address Override. 
/// If this struct exists, the contained physical address
/// should be used in place of the local APIC physical address
/// specified in the MADT ACPI table itself.
#[derive(Copy, Clone, Debug, FromBytes)]
#[repr(packed)]
pub struct MadtLocalApicAddressOverride {
    _header: EntryRecord,
    _reserved: u16,
    /// Local APIC physical address
    pub phys_addr: u64,
}
const _: () = assert!(core::mem::size_of::<MadtLocalApicAddressOverride>() == 12);
const _: () = assert!(core::mem::align_of::<MadtLocalApicAddressOverride>() == 1);

/// Handles the BSP's (bootstrap processor, the first core to boot) entry in the given MADT iterator.
/// This should be the first function invoked to initialize the BSP information, 
/// and should come before any other entries in the MADT are handled.
fn handle_bsp_lapic_entry(madt_iter: MadtIter, page_table: &mut PageTable) -> Result<(), &'static str> {
    use pic::IRQ_BASE_OFFSET;

    for madt_entry in madt_iter.clone() {
        if let MadtEntry::LocalApic(lapic_entry) = madt_entry { 
            let (nmi_lint, nmi_flags) = find_nmi_entry_for_processor(lapic_entry.processor, madt_iter.clone());

            match LocalApic::init(
                page_table,
                lapic_entry.processor,
                None, // we don't know the hardware-assigned APIC ID of the BSP (this CPU) yet
                true,
                nmi_lint,
                nmi_flags,
            ) {
                // this `lapic_entry` wasn't for the BSP, try the next one.
                Err(LapicInitError::NotBSP) => continue,
                Err(other_err) => return Err(Box::leak(format!("{other_err:?}").into_boxed_str())),
                Ok(()) => { } // fall through
            };

            assert!(current_cpu() == lapic_entry.apic_id);
            let bsp_id = lapic_entry.apic_id;

            // redirect every IoApic's interrupts to the one BSP
            // TODO FIXME: I'm unsure if this is actually correct!
            for ioapic in ioapic::get_ioapics().iter() {
                let mut ioapic_ref = ioapic.1.lock();

                // Set the BSP to receive regular PIC interrupts routed through the IoApic.
                // Skip irq 2, since in the PIC that's the chained one (cascade line from PIC2 to PIC1) that isn't used.
                for irq in (0x0 ..= 0x1).chain(0x3 ..= 0xF) {
                    ioapic_ref.set_irq(irq, bsp_id, IRQ_BASE_OFFSET + irq);
                }

                // ioapic_ref.set_irq(0x1, 0xFF, IRQ_BASE_OFFSET + 0x1); 
                // FIXME: the above line does indeed send the interrupt to all cores, but then they all handle it, instead of just one. 
            }
            // there's only ever one BSP, so we can exit the loop here
            break;
        }
    }

    let bsp_id = bootstrap_cpu().ok_or("handle_bsp_lapic_entry(): Couldn't find BSP LocalApic in Madt!")?;

    // now that we've established the BSP, go through the interrupt source override entries
    for madt_entry in madt_iter {
        if let MadtEntry::IntSrcOverride(int_src) = madt_entry {
            let mut handled = false;

            // find the IoApic that should handle this interrupt source override entry
            for (_id, ioapic) in ioapic::get_ioapics().iter() {
                let mut ioapic_ref = ioapic.lock();
                if ioapic_ref.handles_irq(int_src.gsi) {
                    // using BSP for now, but later we could redirect the IRQ to more (or all) cores
                    ioapic_ref.set_irq(int_src.irq_source, bsp_id, int_src.gsi as u8 + IRQ_BASE_OFFSET); 
                    trace!("MadtIntSrcOverride (bus: {}, irq: {}, gsi: {}, flags {:#X}) handled by IoApic {}",
                        int_src.bus_source, int_src.irq_source, &{ int_src.gsi }, &{ int_src.flags }, ioapic_ref.id
                    );
                    handled = true;
                }
            }

            if !handled {
                error!("MadtIntSrcOverride (bus: {}, irq: {}, gsi: {}, flags {:#X}) not handled by any IoApic!",
                    int_src.bus_source, int_src.irq_source, &{ int_src.gsi }, &{ int_src.flags}
                );
            }
        }
    }
    Ok(())
}


/// Handles the IOAPIC entries in the given MADT iterator 
/// by creating IoApic instances for them and initializing them appropriately.
fn handle_ioapic_entries(madt_iter: MadtIter, page_table: &mut PageTable) -> Result<(), &'static str> {
    for madt_entry in madt_iter {
        if let MadtEntry::IoApic(ioa) = madt_entry {
            ioapic::IoApic::create(page_table, ioa.id, PhysicalAddress::new_canonical(ioa.address as usize), ioa.gsi_base)?;
        }
    }
    Ok(())
}


/// Finds the Non-Maskable Interrupt (NMI) entry in the MADT ACPI table (i.e., the given `MadtIter`)
/// corresponding to the given processor. 
/// If no entry exists, it returns the default NMI entry value: `(lint = 1, flags = 0)`.
pub fn find_nmi_entry_for_processor(processor: u8, madt_iter: MadtIter) -> (u8, u16) {
    for madt_entry in madt_iter {
        if let MadtEntry::NonMaskableInterrupt(nmi) = madt_entry {
            // NMI entries are based on the "processor" id, not the "apic_id"
            // Return this Nmi entry if it's for the given lapic, or if it's for all lapics
            if nmi.processor == processor || nmi.processor == 0xFF  {
                return (nmi.lint, nmi.flags);
            }
        }
    }

    let (lint, flags) = (1, 0);
    warn!("Couldn't find NMI entry for processor {} (<-- not apic_id). Using default lint {}, flags {}", processor, lint, flags);
    (lint, flags)
}
