#![no_std]

#![allow(dead_code)]

#[macro_use] extern crate log;
extern crate alloc;
extern crate spin;
extern crate port_io;

use core::fmt;
use core::ops::{Deref, DerefMut};
use alloc::vec::Vec;
use port_io::Port;
use spin::{Once, Mutex};


// The below constants define the PCI configuration space. 
// More info here: <http://wiki.osdev.org/PCI#PCI_Device_Structure>
pub const PCI_VENDOR_ID:             u16 = 0x0;
pub const PCI_DEVICE_ID:             u16 = 0x2;
pub const PCI_COMMAND:               u16 = 0x4;
pub const PCI_STATUS:                u16 = 0x6;
pub const PCI_REVISION_ID:           u16 = 0x8;
pub const PCI_PROG_IF:               u16 = 0x9;
pub const PCI_SUBCLASS:              u16 = 0xA;
pub const PCI_CLASS:                 u16 = 0xB;
pub const PCI_CACHE_LINE_SIZE:       u16 = 0xC;
pub const PCI_LATENCY_TIMER:         u16 = 0xD;
pub const PCI_HEADER_TYPE:           u16 = 0xE;
pub const PCI_BIST:                  u16 = 0xF;
pub const PCI_BAR0:                  u16 = 0x10;
pub const PCI_BAR1:                  u16 = 0x14;
pub const PCI_BAR2:                  u16 = 0x18;
pub const PCI_BAR3:                  u16 = 0x1C;
pub const PCI_BAR4:                  u16 = 0x20;
pub const PCI_BAR5:                  u16 = 0x24;
pub const PCI_CARDBUS_CIS:           u16 = 0x28;
pub const PCI_SUBSYSTEM_VENDOR_ID:   u16 = 0x2C;
pub const PCI_SUBSYSTEM_ID:          u16 = 0x2E;
pub const PCI_EXPANSION_ROM_BASE:    u16 = 0x30;
pub const PCI_CAPABILITIES:          u16 = 0x34;
// 0x35 through 0x3B are reserved
pub const PCI_INTERRUPT_LINE:        u16 = 0x3C;
pub const PCI_INTERRUPT_PIN:         u16 = 0x3D;
pub const PCI_MIN_GRANT:             u16 = 0x3E;
pub const PCI_MAX_LATENCY:           u16 = 0x3F;


/// The maximum number of PCI buses.
const MAX_NUM_PCI_BUSES: u16 = 256;
/// The maximum number of PCI slots on one PCI bus.
const MAX_SLOTS_PER_BUS: u16 = 32;
/// The maximum number of PCI functions (individual devices) on one PCI slot.
const MAX_FUNCTIONS_PER_SLOT: u16 = 8;

/// Addresses/offsets into the PCI configuration space should clear the least-significant 2 bits.
const PCI_CONFIG_ADDRESS_OFFSET_MASK: u16 = 0xFC; 
const CONFIG_ADDRESS: u16 = 0xCF8;
const CONFIG_DATA: u16 = 0xCFC;

static PCI_CONFIG_ADDRESS_PORT: Mutex<Port<u32>> = Mutex::new(Port::new(CONFIG_ADDRESS));
static PCI_CONFIG_DATA_PORT: Mutex<Port<u32>> = Mutex::new(Port::new(CONFIG_DATA));



/// Returns a list of all PCI buses in this system.
/// If the PCI bus hasn't been initialized, this initializes the PCI bus & scans it to enumerates devices.
pub fn get_pci_buses() -> &'static Vec<PciBus> {
    static PCI_BUSES: Once<Vec<PciBus>> = Once::new();
    PCI_BUSES.call_once( || scan_pci() )
}


/// Returns a reference to the `PciDevice` with the given bus, slot, func identifier.
/// If the PCI bus hasn't been initialized, this initializes the PCI bus & scans it to enumerates devices.
pub fn get_pci_device_bsf(bus: u16, slot: u16, func: u16) -> Option<&'static PciDevice> {
    for b in get_pci_buses() {
        if b.bus_number == bus {
            for d in &b.devices {
                if d.slot == slot && d.func == func {
                    return Some(&d);
                }
            }
        }
    }
    None
}


/// Returns an iterator that iterates over all `PciDevice`s, in no particular guaranteed order. 
/// If the PCI bus hasn't been initialized, this initializes the PCI bus & scans it to enumerates devices.
pub fn pci_device_iter() -> impl Iterator<Item = &'static PciDevice> {
    get_pci_buses().iter().flat_map(|b| b.devices.iter())
}


/// A PCI bus, which contains a list of PCI devices on that bus.
#[derive(Debug)]
pub struct PciBus {
    /// The number identifier of this PCI bus.
    pub bus_number: u16,
    /// The list of devices attached to this PCI bus.
    pub devices: Vec<PciDevice>,
}


/// Scans all PCI Buses (brute force iteration) to enumerate PCI Devices on each bus.
/// Initializes structures containing this information. 
fn scan_pci() -> Vec<PciBus> {
	let mut buses: Vec<PciBus> = Vec::new();

    for bus in 0..MAX_NUM_PCI_BUSES {
        let mut device_list: Vec<PciDevice> = Vec::new();

        for slot in 0..MAX_SLOTS_PER_BUS {
            let loc_zero = PciLocation { bus, slot, func: 0 };
            // skip the whole slot if the vendor ID is 0xFFFF
            if 0xFFFF == loc_zero.pci_read_16(PCI_VENDOR_ID) {
                continue;
            }

            // If the header's MSB is set, then there are multiple functions for this device,
            // and we should check all 8 of them to be sure.
            // Otherwise, we only need to check the first function, because it's a single-function device.
            let header_type = loc_zero.pci_read_8(PCI_HEADER_TYPE);
            let functions_to_check = if header_type & 0x80 == 0x80 {
                0..MAX_FUNCTIONS_PER_SLOT
            } else {
                0..1
            };

            for f in functions_to_check {
                let location = PciLocation { bus, slot, func: f };
                let vendor_id = location.pci_read_16(PCI_VENDOR_ID);
                if vendor_id == 0xFFFF {
                    continue;
                }

                let device = PciDevice {
                    vendor_id:        vendor_id,
                    device_id:        location.pci_read_16(PCI_DEVICE_ID), 
                    command:          location.pci_read_16(PCI_COMMAND),
                    status:           location.pci_read_16(PCI_STATUS),
                    revision_id:      location.pci_read_8( PCI_REVISION_ID),
                    prog_if:          location.pci_read_8( PCI_PROG_IF),
                    subclass:         location.pci_read_8( PCI_SUBCLASS),
                    class:            location.pci_read_8( PCI_CLASS),
                    cache_line_size:  location.pci_read_8( PCI_CACHE_LINE_SIZE),
                    latency_timer:    location.pci_read_8( PCI_LATENCY_TIMER),
                    header_type:      location.pci_read_8( PCI_HEADER_TYPE),
                    bist:             location.pci_read_8( PCI_BIST),
                    bars:             [
                                          location.pci_read_32(PCI_BAR0),
                                          location.pci_read_32(PCI_BAR1), 
                                          location.pci_read_32(PCI_BAR2), 
                                          location.pci_read_32(PCI_BAR3), 
                                          location.pci_read_32(PCI_BAR4), 
                                          location.pci_read_32(PCI_BAR5), 
                                      ],
                    int_pin:          location.pci_read_8(PCI_INTERRUPT_PIN),
                    int_line:         location.pci_read_8(PCI_INTERRUPT_LINE),
                    location:              location,
                };

                device_list.push(device);
            }
        }

        if !device_list.is_empty() {
            buses.push( PciBus {
                bus_number: bus, 
                devices: device_list,
            });
        }
    }

    buses	
}


/// The bus, slot, and function number of a given PCI device.
/// This offers methods for reading and writing the PCI config space. 
#[derive(Copy, Clone)]
pub struct PciLocation {
    bus:  u16,
    slot: u16,
    func: u16,
}

impl PciLocation {
    pub fn bus(&self) -> u16 { self.bus }
    pub fn slot(&self) -> u16 { self.slot }
    pub fn function(&self) -> u16 { self.func }


    /// Computes a PCI address from bus, slot, func, and offset. 
    /// The least two significant bits of offset are masked, so it's 4-byte aligned addressing,
    /// which makes sense since we read PCI data (from the configuration space) in 32-bit chunks.
    fn pci_address(self, offset: u16) -> u32 {
        ((self.bus  as u32) << 16) | 
        ((self.slot as u32) << 11) | 
        ((self.func as u32) <<  8) | 
        ((offset as u32) & (PCI_CONFIG_ADDRESS_OFFSET_MASK as u32)) | 
        0x8000_0000
    }

    /// read 32-bit data at the specified `offset` from the PCI device specified by the given `bus`, `slot`, `func` set.
    pub fn pci_read_32(&self, offset: u16) -> u32 {
        unsafe { 
            PCI_CONFIG_ADDRESS_PORT.lock().write(self.pci_address(offset)); 
        }
        PCI_CONFIG_DATA_PORT.lock().read() >> ((offset & (!PCI_CONFIG_ADDRESS_OFFSET_MASK)) * 8)
    }

    /// Read 16-bit data at the specified `offset` from this PCI device.
    pub fn pci_read_16(&self, offset: u16) -> u16 {
        self.pci_read_32(offset) as u16
    } 

    /// Read 8-bit data at the specified `offset` from the PCI device.
    pub fn pci_read_8(&self, offset: u16) -> u8 {
        self.pci_read_32(offset) as u8
    }

    /// Write 32-bit data to the specified `offset` for the PCI device.
    pub fn pci_write(&self, offset: u16, value: u32) {
        unsafe { 
            PCI_CONFIG_ADDRESS_PORT.lock().write(self.pci_address(offset)); 
            PCI_CONFIG_DATA_PORT.lock().write((value) << ((offset & 2) * 8));
        }
    }

    /// Sets the PCI device's bit 3 in the command portion, which is apparently needed to activate DMA (??)
    pub fn pci_set_command_bus_master_bit(&self) {
        unsafe { 
            PCI_CONFIG_ADDRESS_PORT.lock().write(self.pci_address(PCI_COMMAND));
        }
        let inval = PCI_CONFIG_DATA_PORT.lock().read(); 
        trace!("pci_set_command_bus_master_bit: PciDevice: {}, read value: {:#x}", self, inval);
        unsafe {
            PCI_CONFIG_DATA_PORT.lock().write(inval | (1 << 2));
        }
        trace!("pci_set_command_bus_master_bit: PciDevice: {}, read value AFTER WRITE CMD: {:#x}", 
            self,
            PCI_CONFIG_DATA_PORT.lock().read()
        );
    }
}

impl fmt::Display for PciLocation {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(f, "b{}.s{}.f{}", self.bus, self.slot, self.func)
    }
}

impl fmt::Debug for PciLocation {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(f, "{}", self)
    }
}


/// Contains information common to every type of PCI Device,
/// and offers functions for reading/writing to the PCI configuration space.
#[derive(Debug)]
pub struct PciDevice {
    /// the bus, slot, and function number that locates this PCI device in the bus tree.
    pub location: PciLocation,

    /// The class code, used to determine device type: http://wiki.osdev.org/PCI#Class_Codes 
    pub class: u8,
    /// The subclass code, used to determine device type: http://wiki.osdev.org/PCI#Class_Codes 
    pub subclass: u8,
    /// The programming interface of this PCI device
    pub prog_if: u8,
    /// The six Base Address Registers (BARs)
    pub bars: [u32; 6],
    pub vendor_id: u16,
    pub device_id: u16, 
    pub command: u16,
    pub status: u16,
    pub revision_id: u8,
    pub cache_line_size: u8,
    pub latency_timer: u8,
    pub header_type: u8,
    pub bist: u8,
    pub int_pin: u8,
    pub int_line: u8,
}

impl Deref for PciDevice {
    type Target = PciLocation;
    fn deref(&self) -> &PciLocation {
        &self.location
    }
}
impl DerefMut for PciDevice {
    fn deref_mut(&mut self) -> &mut PciLocation {
        &mut self.location
    }
}
