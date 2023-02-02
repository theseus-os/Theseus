#![no_std]

#![allow(dead_code)]

#[macro_use] extern crate log;
extern crate alloc;
extern crate spin;
extern crate port_io;
extern crate memory;
extern crate bit_field;

use core::fmt;
use core::ops::{Deref, DerefMut};
use alloc::vec::Vec;
use port_io::Port;
use spin::{Once, Mutex};
use memory::PhysicalAddress;
use bit_field::BitField;

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

// PCI Capability IDs
pub const MSI_CAPABILITY:           u16 = 0x05;
pub const MSIX_CAPABILITY:          u16 = 0x11;

/// If a BAR's bits [2:1] equal this value, that BAR describes a 64-bit address.
/// If not, that BAR describes a 32-bit address.
const BAR_ADDRESS_IS_64_BIT: u32 = 2;

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
    PCI_BUSES.call_once(scan_pci)
}


/// Returns a reference to the `PciDevice` with the given bus, slot, func identifier.
/// If the PCI bus hasn't been initialized, this initializes the PCI bus & scans it to enumerates devices.
pub fn get_pci_device_bsf(bus: u16, slot: u16, func: u16) -> Option<&'static PciDevice> {
    for b in get_pci_buses() {
        if b.bus_number == bus {
            for d in &b.devices {
                if d.slot == slot && d.func == func {
                    return Some(d);
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
                    vendor_id,
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
                    location,
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
#[derive(Copy, Clone, PartialEq, Eq, Hash)]
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

    /// Sets the PCI device's command bit 10 to disable legacy interrupts
    pub fn pci_set_interrupt_disable_bit(&self) {
        unsafe { 
            PCI_CONFIG_ADDRESS_PORT.lock().write(self.pci_address(PCI_COMMAND));
        }
        let command = PCI_CONFIG_DATA_PORT.lock().read(); 
        trace!("pci_set_interrupt_disable_bit: PciDevice: {}, read value: {:#x}", self, command);

        const INTERRUPT_DISABLE: u32 = 1 << 10;
        unsafe {
            PCI_CONFIG_DATA_PORT.lock().write(command | INTERRUPT_DISABLE);
        }
        trace!("pci_set_interrupt_disable_bit: PciDevice: {} read value AFTER WRITE CMD: {:#x}", 
            self, PCI_CONFIG_DATA_PORT.lock().read());
    }

    /// Explores the PCI config space and returns address of requested capability, if present. 
    /// PCI capabilities are stored as a linked list in the PCI config space, 
    /// with each capability storing the pointer to the next capability right after its ID.
    /// The function returns a None value if capabilities are not valid for this device 
    /// or if the requested capability is not present. 
    pub fn find_pci_capability(&self, pci_capability: u16) -> Option<u16> {
        let status = self.pci_read_16(PCI_STATUS);

        // capabilities are only valid if bit 4 of status register is set
        const CAPABILITIES_VALID: u16 = 1 << 4;
        if  status & CAPABILITIES_VALID != 0 {

            // retrieve the capabilities pointer from the pci config space
            let capabilities = self.pci_read_8(PCI_CAPABILITIES);
            // debug!("capabilities pointer: {:#X}", capabilities);

            // mask the bottom 2 bits of the capabilities pointer to find the address of the first capability
            let mut cap_addr = capabilities as u16 & 0xFFFC;

            // the last capability will have its next pointer equal to zero
            let final_capability = 0;

            // iterate through the linked list of capabilities until the requested capability is found or the list reaches its end
            while cap_addr != final_capability {
                // the capability header is a 16 bit value which contains the current capability ID and the pointer to the next capability
                let cap_header = self.pci_read_16(cap_addr);

                // the id is the lower byte of the header
                let cap_id = cap_header & 0xFF;
                
                if cap_id == pci_capability {
                        debug!("Found capability: {:#X} at {:#X}", pci_capability, cap_addr);
                        return Some(cap_addr);
                }

                // find address of next capability which is the higher byte of the header
                cap_addr = (cap_header >> 8) & 0xFF;            
            }
        }
        None
    }
}

impl fmt::Display for PciLocation {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(f, "b{}.s{}.f{}", self.bus, self.slot, self.func)
    }
}

impl fmt::Debug for PciLocation {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(f, "{self}")
    }
}


/// Contains information common to every type of PCI Device,
/// and offers functions for reading/writing to the PCI configuration space.
///
/// For more, see [this partial table](http://wiki.osdev.org/PCI#Class_Codes)
/// of `class`, `subclass`, and `prog_if` codes, 
#[derive(Debug)]
pub struct PciDevice {
    /// the bus, slot, and function number that locates this PCI device in the bus tree.
    pub location: PciLocation,

    /// The class code, used to determine device type.
    pub class: u8,
    /// The subclass code, used to determine device type.
    pub subclass: u8,
    /// The programming interface of this PCI device, also used to determine device type.
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

impl PciDevice {
    /// Returns the base address of the memory region specified by the given `BAR` 
    /// (Base Address Register) for this PCI device. 
    ///
    /// # Argument
    /// * `bar_index` must be between `0` and `5` inclusively, as each PCI device 
    ///   can only have 6 BARs at the most.  
    ///
    /// Note that if the given `BAR` actually indicates it is part of a 64-bit address,
    /// it will be used together with the BAR right above it (`bar + 1`), e.g., `BAR1:BAR0`.
    /// If it is a 32-bit address, then only the given `BAR` will be accessed.
    ///
    /// TODO: currently we assume the BAR represents a memory space (memory mapped I/O) 
    ///       rather than I/O space like Port I/O. Obviously, this is not always the case.
    ///       Instead, we should return an enum specifying which kind of memory space the calculated base address is.
    pub fn determine_mem_base(&self, bar_index: usize) -> Result<PhysicalAddress, &'static str> {
        let mut bar = if let Some(bar_value) = self.bars.get(bar_index) {
            *bar_value
        } else {
            return Err("BAR index must be between 0 and 5 inclusive");
        };

        // Check bits [2:1] of the bar to determine address length (64-bit or 32-bit)
        let mem_base = if bar.get_bits(1..3) == BAR_ADDRESS_IS_64_BIT { 
            // Here: this BAR is the lower 32-bit part of a 64-bit address, 
            // so we need to access the next highest BAR to get the address's upper 32 bits.
            let next_bar = *self.bars.get(bar_index + 1).ok_or("next highest BAR index is out of range")?;
            // Clear the bottom 4 bits because it's a 16-byte aligned address
            PhysicalAddress::new(*bar.set_bits(0..4, 0) as usize | ((next_bar as usize) << 32))
                .ok_or("determine_mem_base(): [64-bit] BAR physical address was invalid")?
        } else {
            // Here: this BAR is the lower 32-bit part of a 64-bit address, 
            // so we need to access the next highest BAR to get the address's upper 32 bits.
            // Also, clear the bottom 4 bits because it's a 16-byte aligned address.
            PhysicalAddress::new(*bar.set_bits(0..4, 0) as usize)
                .ok_or("determine_mem_base(): [32-bit] BAR physical address was invalid")?
        };  
        Ok(mem_base)
    }

    /// Returns the size in bytes of the memory region specified by the given `BAR` 
    /// (Base Address Register) for this PCI device.
    ///
    /// # Argument
    /// * `bar_index` must be between `0` and `5` inclusively, as each PCI device 
    /// can only have 6 BARs at the most. 
    ///
    pub fn determine_mem_size(&self, bar_index: usize) -> u32 {
        assert!(bar_index < 6);
        // Here's what we do: 
        // (1) Write all `1`s to the specified BAR
        // (2) Read that BAR value again
        // (3) Mask the info bits (bits [3:0]) of the BAR value read in Step 2
        // (4) Bitwise "not" (negate) that value, then add 1.
        //     The resulting value is the size of that BAR's memory region.
        // (5) Restore the original value to that BAR
        let bar_offset = PCI_BAR0 + (bar_index as u16 * 0x4);
        let original_value = self.bars[bar_index];

        self.pci_write(bar_offset, 0xFFFF_FFFF);          // Step 1
        let mut mem_size = self.pci_read_32(bar_offset);  // Step 2
        mem_size.set_bits(0..4, 0);                       // Step 3
        mem_size = !(mem_size);                           // Step 4
        mem_size += 1;                                    // Step 4
        self.pci_write(bar_offset, original_value);       // Step 5
        mem_size
    }

    /// Enable MSI interrupts for a PCI device.
    /// We assume the device only supports one MSI vector 
    /// and set the interrupt number and core id for that vector.
    /// If the MSI capability is not supported then an error message is returned.
    /// 
    /// # Arguments
    /// * `core_id`: core that interrupt will be routed to
    /// * `int_num`: interrupt number to assign to the MSI vector
    pub fn pci_enable_msi(&self, core_id: u8, int_num: u8) -> Result<(), &'static str> {

        // find out if the device is msi capable
        let cap_addr = self.find_pci_capability(MSI_CAPABILITY).ok_or("Device not MSI capable")?;

        // offset in the capability space where the message address register is located 
        const MESSAGE_ADDRESS_REGISTER_OFFSET: u16 = 4;
        // the memory region is a constant defined for Intel cpus where MSI messages are written
        // it should be written to bit 20 of the message address register
        const MEMORY_REGION: u32 = 0x0FEE << 20;
        // the core id tells which cpu the interrupt will be routed to 
        // it should be written to bit 12 of the message address register
        let core = (core_id as u32) << 12;
        // set the core the MSI will be sent to in the Message Address Register (Intel Arch SDM, vol3, 10.11)
        self.pci_write(cap_addr + MESSAGE_ADDRESS_REGISTER_OFFSET, MEMORY_REGION| core);

        // offset in the capability space where the message data register is located 
        const MESSAGE_DATA_REGISTER_OFFSET: u16 = 12;
        // Set the interrupt number for the MSI in the Message Data Register
        self.pci_write(cap_addr + MESSAGE_DATA_REGISTER_OFFSET, int_num as u32);

        // offset in the capability space where the message control register is located 
        const MESSAGE_CONTROL_REGISTER_OFFSET: u16 = 2;
        // to enable the MSI capability, we need to set it bit 0 of the message control register
        const MSI_ENABLE: u32 = 1;
        let ctrl = self.pci_read_16(cap_addr + MESSAGE_CONTROL_REGISTER_OFFSET) as u32;
        // enable MSI in the Message Control Register
        self.pci_write(cap_addr + MESSAGE_CONTROL_REGISTER_OFFSET, ctrl | MSI_ENABLE);

        Ok(())  
    }

    /// Enable MSI-X interrupts for a PCI device.
    /// Only the enable bit is set and the remaining initialization steps of
    /// setting the interrupt number and core id should be completed in the device driver.
    pub fn pci_enable_msix(&self) -> Result<(), &'static str> {

        // find out if the device is msi-x capable
        let cap_addr = self.find_pci_capability(MSIX_CAPABILITY).ok_or("Device not MSI-X capable")?;

        // offset in the capability space where the message control register is located 
        const MESSAGE_CONTROL_REGISTER_OFFSET: u16 = 2;
        let ctrl = self.pci_read_16(cap_addr + MESSAGE_CONTROL_REGISTER_OFFSET) as u32;

        // write to bit 15 of Message Control Register to enable MSI-X 
        const MSIX_ENABLE: u32 = 1 << 15; 
        self.pci_write(cap_addr + MESSAGE_CONTROL_REGISTER_OFFSET, ctrl | MSIX_ENABLE);

        // let ctrl = pci_read_32(dev.bus, dev.slot, dev.func, cap_addr);
        // debug!("MSIX HEADER AFTER ENABLE: {:#X}", ctrl);

        Ok(())  
    }
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



/// Lists the 2 possible PCI configuration space access mechanisms
/// that can be found from the LSB of the devices's BAR0
pub enum PciConfigSpaceAccessMechanism {
    MemoryMapped = 0,
    IoPort = 1,
}