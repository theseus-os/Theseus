//! PCI Configuration Space Access
//!
//! Note: while pci currently uses port-io on x86 and mmio on aarch64,
//! x86 may also support memory-based PCI configuration in the future;
//! port-io is the legacy way to access the config space.

#![no_std]
#![allow(dead_code)]

extern crate alloc;

use log::*;
use core::{fmt, ops::{Deref, DerefMut}, mem::size_of};
use alloc::vec::Vec;
use spin::{Once, Mutex};
use memory::{PhysicalAddress, BorrowedSliceMappedPages, Mutable, MappedPages, map_frame_range, MMIO_FLAGS};
use bit_field::BitField;
use volatile::Volatile;
use zerocopy::FromBytes;
use cpu::CpuId;
use interrupts::InterruptNumber;

#[cfg(target_arch = "x86_64")]
use port_io::Port;

#[cfg(target_arch = "aarch64")]
use arm_boards::BOARD_CONFIG;

#[derive(Debug, Copy, Clone)]
/// Defines a PCI register as part of a dword
enum RegisterSpan {
    /// Bits [0:31]
    FullDword,
    /// Bits [0:15]
    Word0,
    /// Bits [16:31]
    Word1,
    /// Bits [0:7]
    Byte0,
    /// Bits [8:15]
    Byte1,
    /// Bits [16:23]
    Byte2,
    /// Bits [24:31]
    Byte3,
}

use RegisterSpan::*;

/// (PCI Config Space Dword Index, Register Span)
type PciRegister = (u8, RegisterSpan);

/// Creates a byte register definition from a byte offset
const fn offset_to_byte_reg_def(offset: u8) -> PciRegister {
    let reg_index = offset >> 2;
    let reg_span = match offset & 0b11 {
        0 => Byte0,
        1 => Byte1,
        2 => Byte2,
        3 => Byte3,
        _ => unreachable!(),
    };
    (reg_index, reg_span)
}

// The below constants define the PCI configuration space. 
// More info here: <http://wiki.osdev.org/PCI#PCI_Device_Structure>
const PCI_VENDOR_ID:             PciRegister = (0x0, Word0);
const PCI_DEVICE_ID:             PciRegister = (0x0, Word1);
const PCI_COMMAND:               PciRegister = (0x1, Word0);
const PCI_STATUS:                PciRegister = (0x1, Word1);
const PCI_REVISION_ID:           PciRegister = (0x2, Byte0);
const PCI_PROG_IF:               PciRegister = (0x2, Byte1);
const PCI_SUBCLASS:              PciRegister = (0x2, Byte2);
const PCI_CLASS:                 PciRegister = (0x2, Byte3);
const PCI_CACHE_LINE_SIZE:       PciRegister = (0x3, Byte0);
const PCI_LATENCY_TIMER:         PciRegister = (0x3, Byte1);
const PCI_HEADER_TYPE:           PciRegister = (0x3, Byte2);
const PCI_BIST:                  PciRegister = (0x3, Byte3);
const PCI_BAR0:                  PciRegister = (0x4, FullDword);
const PCI_BAR1:                  PciRegister = (0x5, FullDword);
const PCI_BAR2:                  PciRegister = (0x6, FullDword);
const PCI_BAR3:                  PciRegister = (0x7, FullDword);
const PCI_BAR4:                  PciRegister = (0x8, FullDword);
const PCI_BAR5:                  PciRegister = (0x9, FullDword);
const PCI_CARDBUS_CIS:           PciRegister = (0xA, FullDword);
const PCI_SUBSYSTEM_VENDOR_ID:   PciRegister = (0xB, Word0);
const PCI_SUBSYSTEM_ID:          PciRegister = (0xB, Word1);
const PCI_EXPANSION_ROM_BASE:    PciRegister = (0xC, FullDword);
const PCI_CAPABILITIES:          PciRegister = (0xD, Byte0);
// 0x35 through 0x3B are reserved
const PCI_INTERRUPT_LINE:        PciRegister = (0xF, Byte0);
const PCI_INTERRUPT_PIN:         PciRegister = (0xF, Byte1);
const PCI_MIN_GRANT:             PciRegister = (0xF, Byte2);
const PCI_MAX_LATENCY:           PciRegister = (0xF, Byte3);

#[repr(u8)]
pub enum PciCapability {
    Msi  = 0x05,
    Msix = 0x11,
}

/// If a BAR's bits [2:1] equal this value, that BAR describes a 64-bit address.
/// If not, that BAR describes a 32-bit address.
const BAR_ADDRESS_IS_64_BIT: u32 = 2;

/// There is a maximum of 256 PCI buses on one system.
const MAX_PCI_BUSES: u16 = 256;
/// There is a maximum of 32 slots on one PCI bus.
const MAX_SLOTS_PER_BUS: u8 = 32;
/// There is a maximum of 32 functions (devices) on one PCI slot.
const MAX_FUNCTIONS_PER_SLOT: u8 = 8;

/// Addresses/offsets into the PCI configuration space should clear the
/// least-significant 2 bits in order to be 4-byte aligned.
const PCI_CONFIG_ADDRESS_OFFSET_MASK: u8 = 0b11111100;

const CONFIG_ADDRESS: u16 = 0xCF8;
const CONFIG_DATA: u16 = 0xCFC;

/// This port is used to specify the address in the PCI configuration space
/// for the next read/write of the `PCI_CONFIG_DATA_PORT`.
#[cfg(target_arch = "x86_64")]
static PCI_CONFIG_ADDRESS_PORT: Mutex<Port<u32>> = Mutex::new(Port::new(CONFIG_ADDRESS));

/// This port is used to transfer data to or from the PCI configuration space
/// specified by a previous write to the `PCI_CONFIG_ADDRESS_PORT`.
#[cfg(target_arch = "x86_64")]
static PCI_CONFIG_DATA_PORT: Mutex<Port<u32>> = Mutex::new(Port::new(CONFIG_DATA));

#[cfg(target_arch = "x86_64")]
const BASE_OFFSET: u32 = 0x8000_0000;

#[cfg(target_arch = "aarch64")]
type PciConfigSpace = BorrowedSliceMappedPages<Volatile<u32>, Mutable>;

#[cfg(target_arch = "aarch64")]
static PCI_CONFIG_SPACE: Mutex<Once<PciConfigSpace>> = Mutex::new(Once::new());

#[cfg(target_arch = "aarch64")]
const BASE_OFFSET: u32 = 0;

pub enum InterruptPin {
    A,
    B,
    C,
    D,
}



/// Returns a list of all PCI buses in this system.
/// If the PCI bus hasn't been initialized, this initializes the PCI bus & scans it to enumerates devices.
pub fn get_pci_buses() -> Result<&'static Vec<PciBus>, &'static str> {
    static PCI_BUSES: Once<Vec<PciBus>> = Once::new();
    PCI_BUSES.try_call_once(scan_pci)
}


/// Returns a reference to the `PciDevice` with the given bus, slot, func identifier.
/// If the PCI bus hasn't been initialized, this initializes the PCI bus & scans it to enumerates devices.
pub fn get_pci_device_bsf(bus: u8, slot: u8, func: u8) -> Result<Option<&'static PciDevice>, &'static str> {
    for b in get_pci_buses()? {
        if b.bus_number == bus {
            for d in &b.devices {
                if d.slot == slot && d.func == func {
                    return Ok(Some(d));
                }
            }
        }
    }

    Ok(None)
}


/// Returns an iterator that iterates over all `PciDevice`s, in no particular guaranteed order. 
/// If the PCI bus hasn't been initialized, this initializes the PCI bus & scans it to enumerates devices.
pub fn pci_device_iter() -> Result<impl Iterator<Item = &'static PciDevice>, &'static str> {
    Ok(get_pci_buses()?.iter().flat_map(|b| b.devices.iter()))
}


/// A PCI bus, which contains a list of PCI devices on that bus.
#[derive(Debug)]
pub struct PciBus {
    /// The number identifier of this PCI bus.
    pub bus_number: u8,
    /// The list of devices attached to this PCI bus.
    pub devices: Vec<PciDevice>,
}


/// Scans all PCI Buses (brute force iteration) to enumerate PCI Devices on each bus.
/// Initializes structures containing this information. 
fn scan_pci() -> Result<Vec<PciBus>, &'static str> {
    #[cfg(target_arch = "aarch64")]
    PCI_CONFIG_SPACE.lock().try_call_once(|| {
        let config = BOARD_CONFIG.pci_ecam;
        let mapped = memory::map_frame_range(config.base_address, config.size_bytes, MMIO_FLAGS)?;
        let config_space_u32_len = BOARD_CONFIG.pci_ecam.size_bytes / size_of::<u32>();
        match mapped.into_borrowed_slice_mut(0, config_space_u32_len) {
            Ok(bsm) => Ok(bsm),
            Err((_, msg)) => Err(msg),
        }
    })?;

    let mut buses: Vec<PciBus> = Vec::new();

    for bus in 0..MAX_PCI_BUSES {
        let bus = bus as u8;
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

    Ok(buses)   
}

impl RegisterSpan {
    fn to_mask_and_shift(self) -> (u32, u32) {
        match self {
            FullDword => (0xffff_ffff,  0),
            Word0     => (0x0000_ffff,  0),
            Word1     => (0xffff_0000, 16),
            Byte0     => (0x0000_00ff,  0),
            Byte1     => (0x0000_ff00,  8),
            Byte2     => (0x00ff_0000, 16),
            Byte3     => (0xff00_0000, 24),
        }
    }

    fn width_in_bytes(self) -> usize {
        match self {
            FullDword => size_of::<u32>(),
            Word0     => size_of::<u16>(),
            Word1     => size_of::<u16>(),
            Byte0     => size_of::<u8>(),
            Byte1     => size_of::<u8>(),
            Byte2     => size_of::<u8>(),
            Byte3     => size_of::<u8>(),
        }
    }
}

/// The bus, slot, and function number of a given PCI device.
/// This offers methods for reading and writing the PCI config space. 
#[derive(Copy, Clone, PartialEq, Eq, Hash)]
pub struct PciLocation {
    bus:  u8,
    slot: u8,
    func: u8,
}

impl PciLocation {
    pub fn bus(&self) -> u8 { self.bus }
    pub fn slot(&self) -> u8 { self.slot }
    pub fn function(&self) -> u8 { self.func }

    /// Read a register from the PCI Configuration Space.
    fn pci_read_raw(&self, register: PciRegister) -> u32 {
        let (dword_offset, reg_span) = register;
        let (mask, shift) = reg_span.to_mask_and_shift();
        let u32_bytes = size_of::<u32>() as u32;

        let dword_address = BASE_OFFSET
            | ((self.bus     as u32) << 16)
            | ((self.slot    as u32) << 11)
            | ((self.func    as u32) <<  8)
            | ((dword_offset as u32) * u32_bytes);

        let dword_value;

        #[cfg(target_arch = "x86_64")] {
            unsafe { 
                PCI_CONFIG_ADDRESS_PORT.lock().write(dword_address);
            }
            dword_value = PCI_CONFIG_DATA_PORT.lock().read();
        }

        #[cfg(target_arch = "aarch64")] {
            let config_space = PCI_CONFIG_SPACE.lock();
            let config_space = config_space.get()
                .expect("PCI Config Space wasn't mapped yet");
            let dword_index = (dword_address as usize) / size_of::<u32>();
            dword_value = config_space[dword_index].read();
        }

        (dword_value & mask) >> shift
    }

    /// Read a 4-bytes register from the PCI Configuration Space.
    ///
    /// Panics if the register isn't a [`FullDword`]
    fn pci_read_32(&self, register: PciRegister) -> u32 {
        let reg_width = register.1.width_in_bytes();
        let output_width = size_of::<u32>();
        assert_eq!(reg_width, output_width, "pci_read_32: register isn't 32-bit wide");

        self.pci_read_raw(register)
    }

    /// Read a 2-bytes register from the PCI Configuration Space.
    ///
    /// Panics if the register isn't a [`Word0`] / [`Word1`]
    fn pci_read_16(&self, register: PciRegister) -> u16 {
        let reg_width = register.1.width_in_bytes();
        let output_width = size_of::<u16>();
        assert_eq!(reg_width, output_width, "pci_read_16: register isn't 16-bit wide");

        self.pci_read_raw(register) as _
    }

    /// Read a one-byte register from the PCI Configuration Space.
    ///
    /// Panics if the register isn't a [`Byte0`] / [`Byte1`] / [`Byte2`] / [`Byte3`]
    fn pci_read_8(&self, register: PciRegister) -> u8 {
        let reg_width = register.1.width_in_bytes();
        let output_width = size_of::<u8>();
        assert_eq!(reg_width, output_width, "pci_read_16: register isn't 8-bit wide");

        self.pci_read_raw(register) as _
    }

    /// Write to a register in the PCI Configuration Space.
    fn pci_write_raw(&self, register: PciRegister, value: u32) {
        let (dword_offset, reg_span) = register;
        let (mask, shift) = reg_span.to_mask_and_shift();
        let u32_bytes = size_of::<u32>() as u32;

        let dword_address = BASE_OFFSET
            | ((self.bus  as u32) << 16)
            | ((self.slot as u32) << 11)
            | ((self.func as u32) <<  8)
            | ((dword_offset as u32) * u32_bytes);

        #[cfg(target_arch = "x86_64")] {
            unsafe {
                PCI_CONFIG_ADDRESS_PORT.lock().write(dword_address); 

                let mut dword = PCI_CONFIG_DATA_PORT.lock().read();
                dword &= !mask;
                dword |= value << shift;
                PCI_CONFIG_DATA_PORT.lock().write(dword);
            }
        }

        #[cfg(target_arch = "aarch64")] {
            let mut config_space = PCI_CONFIG_SPACE.lock();
            let config_space = config_space.get_mut()
                .expect("PCI Config Space wasn't mapped yet");
            let dword_index = (dword_address as usize) / size_of::<u32>();

            let mut dword = config_space[dword_index].read();
            dword &= !mask;
            dword |= value << shift;
            config_space[dword_index].write(dword);
        }
    }

    /// Write a 4-bytes register from the PCI Configuration Space.
    ///
    /// Panics if the register isn't a [`FullDword`]
    fn pci_write_32(&self, register: PciRegister, value: u32) {
        let reg_width = register.1.width_in_bytes();
        let output_width = size_of::<u32>();
        assert_eq!(reg_width, output_width, "pci_write_32: register isn't 32-bit wide");

        self.pci_write_raw(register, value)
    }

    /// Write a 2-bytes register from the PCI Configuration Space.
    ///
    /// Panics if the register isn't a [`Word0`] / [`Word1`]
    fn pci_write_16(&self, register: PciRegister, value: u16) {
        let reg_width = register.1.width_in_bytes();
        let output_width = size_of::<u16>();
        assert_eq!(reg_width, output_width, "pci_write_16: register isn't 16-bit wide");

        self.pci_write_raw(register, value as _)
    }

    /// Write a one-byte register from the PCI Configuration Space.
    ///
    /// Panics if the register isn't a [`Byte0`] / [`Byte1`] / [`Byte2`] / [`Byte3`]
    fn pci_write_8(&self, register: PciRegister, value: u8) {
        let reg_width = register.1.width_in_bytes();
        let output_width = size_of::<u8>();
        assert_eq!(reg_width, output_width, "pci_write_16: register isn't 8-bit wide");

        self.pci_write_raw(register, value as _)
    }

    /// Sets the PCI device's bit 3 in the command portion, which is apparently needed to activate DMA (??)
    pub fn pci_set_command_bus_master_bit(&self) {
        let value = self.pci_read_16(PCI_COMMAND);
        trace!("pci_set_command_bus_master_bit: PciDevice: {}, read value: {:#x}", self, value);

        self.pci_write_16(PCI_COMMAND, value | (1 << 2));

        trace!("pci_set_command_bus_master_bit: PciDevice: {}, read value AFTER WRITE CMD: {:#x}", 
            self,
            self.pci_read_16(PCI_COMMAND),
        );
    }

    /// Sets the PCI device's command bit 10 to disable legacy interrupts
    pub fn pci_set_interrupt_disable_bit(&self) {
        let command = self.pci_read_16(PCI_COMMAND);
        trace!("pci_set_interrupt_disable_bit: PciDevice: {}, read value: {:#x}", self, command);

        const INTERRUPT_DISABLE: u16 = 1 << 10;
        self.pci_write_16(PCI_COMMAND, command | INTERRUPT_DISABLE);

        trace!("pci_set_interrupt_disable_bit: PciDevice: {} read value AFTER WRITE CMD: {:#x}", 
            self,
            self.pci_read_16(PCI_COMMAND),
        );
    }

    /// Explores the PCI config space and returns address of requested capability, if present.
    /// PCI capabilities are stored as a linked list in the PCI config space,
    /// with each capability storing the pointer to the next capability right after its ID.
    /// The function returns a None value if capabilities are not valid for this device
    /// or if the requested capability is not present.
    fn find_pci_capability(&self, pci_capability: PciCapability) -> Option<u8> {
        let pci_capability = pci_capability as u8;
        let status = self.pci_read_16(PCI_STATUS);

        // capabilities are only valid if bit 4 of status register is set
        const CAPABILITIES_VALID: u16 = 1 << 4;
        if status & CAPABILITIES_VALID != 0 {

            // retrieve the capabilities pointer from the pci config space
            let capabilities = self.pci_read_8(PCI_CAPABILITIES);
            // debug!("capabilities pointer: {:#X}", capabilities);

            // mask the bottom 2 bits of the capabilities pointer to find the address of the first capability
            let mut cap_addr = capabilities & 0xFC;

            // the last capability will have its next pointer equal to zero
            let final_capability = 0;

            // iterate through the linked list of capabilities until the requested
            // capability is found or the list reaches its end
            while cap_addr != final_capability {
                // the capability header is a 16 bit value which contains
                // the current capability ID and the pointer to the next capability

                // the id is the lower byte of the header
                let cap_id_reg = offset_to_byte_reg_def(cap_addr);
                let cap_id = self.pci_read_8(cap_id_reg);

                if cap_id == pci_capability {
                    debug!("Found capability: {:#X} at {:#X}", pci_capability, cap_addr);
                    return Some(cap_addr);
                }

                // find address of next capability which is the higher byte of the header
                let next_cap_ptr_reg = offset_to_byte_reg_def(cap_addr + 1);
                cap_addr = self.pci_read_8(next_cap_ptr_reg);
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
        let bar_reg_def = (PCI_BAR0.0 + (bar_index as u8), FullDword);
        let original_value = self.bars[bar_index];

        self.pci_write_32(bar_reg_def, 0xFFFF_FFFF);       // Step 1
        let mut mem_size = self.pci_read_32(bar_reg_def);  // Step 2
        mem_size.set_bits(0..4, 0);                        // Step 3
        mem_size = !(mem_size);                            // Step 4
        mem_size += 1;                                     // Step 4
        self.pci_write_32(bar_reg_def, original_value);    // Step 5
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
    ///
    /// # Panics
    ///
    /// This function panics if the MSI capability isn't aligned to 4 bytes
    pub fn pci_enable_msi(&self, core_id: u8, int_num: u8) -> Result<(), &'static str> {

        // find out if the device is msi capable
        let cap_addr = self.find_pci_capability(PciCapability::Msi).ok_or("Device not MSI capable")?;
        assert_eq!(cap_addr & 0b11, 0, "pci_enable_msi: Invalid MSI capability address alignment");
        let msi_reg_index = cap_addr >> 2;

        // offset in the capability space where the message address register is located 
        const MESSAGE_ADDRESS_REGISTER_OFFSET: u8 = 1 /* one dword */;

        // the memory region is a constant defined for Intel cpus where MSI messages are written
        // it should be written to bit 20 of the message address register
        const MEMORY_REGION: u32 = 0x0FEE << 20;

        // the core id tells which cpu the interrupt will be routed to 
        // it should be written to bit 12 of the message address register
        let core = (core_id as u32) << 12;

        // set the core the MSI will be sent to in the Message Address Register (Intel Arch SDM, vol3, 10.11)
        let reg_def = (msi_reg_index + MESSAGE_ADDRESS_REGISTER_OFFSET, FullDword);
        self.pci_write_32(reg_def, MEMORY_REGION | core);

        // offset in the capability space where the message data register is located 
        const MESSAGE_DATA_REGISTER_OFFSET: u8 = 3 /* dwords */;

        // Set the interrupt number for the MSI in the Message Data Register
        let reg_def = (msi_reg_index + MESSAGE_DATA_REGISTER_OFFSET, FullDword);
        self.pci_write_32(reg_def, int_num as u32);

        // to enable the MSI capability, we need to set bit 0 of the message control register
        const MSI_ENABLE: u16 = 1;

        // enable MSI in the Message Control Register
        // the message control register corresponds to bits [16:31] of the first dword
        let reg_def = (msi_reg_index, Word1);
        let mut ctrl = self.pci_read_16(reg_def);
        ctrl |= MSI_ENABLE;
        self.pci_write_16(reg_def, ctrl);

        Ok(())  
    }

    /// Enable MSI-X interrupts for a PCI device.
    /// Only the enable bit is set and the remaining initialization steps of
    /// setting the interrupt number and core id should be completed in the device driver.
    pub fn pci_enable_msix(&self) -> Result<(), &'static str> {
        // find out if the device is msi-x capable
        let cap_addr = self.find_pci_capability(PciCapability::Msix).ok_or("Device not MSI-X capable")?;
        assert_eq!(cap_addr & 0b11, 0, "pci_enable_msix: Invalid MSI-X capability address alignment");
        let msix_reg_index = cap_addr >> 2;

        // write to bit 15 of Message Control Register to enable MSI-X
        const MSIX_ENABLE: u16 = 1 << 15;

        // the message control register corresponds to bits [16:31] of the first dword
        let reg_def = (msix_reg_index, Word1);
        let mut ctrl = self.pci_read_16(reg_def);
        ctrl |= MSIX_ENABLE;
        self.pci_write_16(reg_def, ctrl);

        // let ctrl = pci_read_16(reg_def, msix_reg_index);
        // debug!("MSIX HEADER AFTER ENABLE: {:#X}", ctrl);

        Ok(())  
    }

    /// Returns the memory mapped msix vector table
    ///
    /// - returns `Err("Device not MSI-X capable")` if the device doesn't have the MSI-X capability
    /// - returns `Err("Invalid BAR content")` if the Base Address Register contains an invalid address
    pub fn pci_mem_map_msix(&self, max_vectors: usize) -> Result<MsixVectorTable, &'static str> {
        // retreive the address in the pci config space for the msi-x capability
        let cap_addr = self.find_pci_capability(PciCapability::Msix).ok_or("Device not MSI-X capable")?;
        assert_eq!(cap_addr & 0b11, 0, "pci_enable_msix: Invalid MSI-X capability address alignment");
        let msix_reg_index = cap_addr >> 2;

        // get physical address of vector table
        let vector_table_offset = 1 /* one dword */;
        let reg_def = (msix_reg_index + vector_table_offset, FullDword);
        let table_offset = self.pci_read_32(reg_def);
        let bar = table_offset & 0x7;
        let offset = table_offset >> 3;
        let addr = self.bars[bar as usize] + offset;

        // find the memory base address and size of the area for the vector table
        let mem_base = PhysicalAddress::new(addr as usize).ok_or("Invalid BAR content")?;
        let mem_size_in_bytes = size_of::<MsixVectorEntry>() * max_vectors;

        // debug!("msi-x vector table bar: {}, base_address: {:#X} and size: {} bytes", bar, mem_base, mem_size_in_bytes);

        let msix_mapped_pages = map_frame_range(mem_base, mem_size_in_bytes, MMIO_FLAGS)?;
        let vector_table = BorrowedSliceMappedPages::from_mut(msix_mapped_pages, 0, max_vectors)
            .map_err(|(_mp, err)| err)?;

        Ok(MsixVectorTable::new(vector_table))
    }

    /// Maps device memory specified by a Base Address Register.
    ///
    /// # Arguments 
    /// * `bar_index`: index of the Base Address Register to use
    pub fn pci_map_bar_mem(&self, bar_index: usize) -> Result<MappedPages, &'static str> {
        let mem_base = self.determine_mem_base(bar_index)?;
        let mem_size = self.determine_mem_size(bar_index);
        map_frame_range(mem_base, mem_size as usize, MMIO_FLAGS)
    }

    /// Reads and returns this PCI device's interrupt line and interrupt pin registers.
    ///
    /// Returns an error if this PCI device's interrupt pin value is invalid (greater than 4).
    pub fn pci_get_interrupt_info(&self) -> Result<(Option<u8>, Option<InterruptPin>), &'static str> {
        let int_line = match self.pci_read_8(PCI_INTERRUPT_LINE) {
            0xff => None,
            other => Some(other),
        };

        let int_pin = match self.pci_read_8(PCI_INTERRUPT_PIN) {
            0 => None,
            1 => Some(InterruptPin::A),
            2 => Some(InterruptPin::B),
            3 => Some(InterruptPin::C),
            4 => Some(InterruptPin::D),
            _ => return Err("pci_get_interrupt_info: Invalid Register Value for Interrupt Pin"),
        };

        Ok((int_line, int_pin))
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

/// A memory-mapped array of [`MsixVectorEntry`]
pub struct MsixVectorTable {
    entries: BorrowedSliceMappedPages<MsixVectorEntry, Mutable>,
}

impl MsixVectorTable {
    pub fn new(entries: BorrowedSliceMappedPages<MsixVectorEntry, Mutable>) -> Self {
        Self { entries }
    }
}
impl Deref for MsixVectorTable {
    type Target = [MsixVectorEntry];
    fn deref(&self) -> &Self::Target {
        &self.entries
    }
}
impl DerefMut for MsixVectorTable {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.entries
    }
}

/// A single Message Signaled Interrupt entry.
///
/// This entry contains the interrupt's IRQ vector number
/// and the CPU to which the interrupt will be delivered.
#[derive(FromBytes)]
#[repr(C)]
pub struct MsixVectorEntry {
    /// The lower portion of the address for the memory write transaction.
    /// This part contains the CPU ID which the interrupt will be redirected to.
    msg_lower_addr:         Volatile<u32>,
    /// The upper portion of the address for the memory write transaction.
    msg_upper_addr:         Volatile<u32>,
    /// The data portion of the msi vector which contains the interrupt number.
    msg_data:               Volatile<u32>,
    /// The control portion which contains the interrupt mask bit.
    vector_control:         Volatile<u32>,
}

impl MsixVectorEntry {
    /// Sets interrupt destination & number for this entry and makes sure the
    /// interrupt is unmasked (PCI Controller side).
    pub fn init(&mut self, cpu_id: CpuId, int_num: InterruptNumber) {
        // unmask the interrupt
        self.vector_control.write(MSIX_UNMASK_INT);
        let lower_addr = self.msg_lower_addr.read();

        // set the CPU to which this interrupt will be delivered.
        let dest_id = (cpu_id.into_u8() as u32) << MSIX_DEST_ID_SHIFT;
        let address = lower_addr & !MSIX_ADDRESS_BITS;
        self.msg_lower_addr.write(address | MSIX_INTERRUPT_REGION | dest_id); 

        // write interrupt number
        #[allow(clippy::unnecessary_cast)]
        self.msg_data.write(int_num as u32);

        if false {
            let control = self.vector_control.read();
            debug!("Created MSI vector: control: {}, CPU: {}, int: {}", control, cpu_id, int_num);
        }
    }
}

/// A constant which indicates the region that is reserved for interrupt messages
const MSIX_INTERRUPT_REGION:    u32 = 0xFEE << 20;
/// The location in the lower address register where the destination CPU ID is written
const MSIX_DEST_ID_SHIFT:       u32 = 12;
/// The bits in the lower address register that need to be cleared and set
const MSIX_ADDRESS_BITS:        u32 = 0xFFFF_FFF0;
/// Clear the vector control field to unmask the interrupt
const MSIX_UNMASK_INT:          u32 = 0;
