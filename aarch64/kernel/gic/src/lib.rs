//! Allows configuring the Generic Interrupt Controller
#![no_std]

use memory::{PageTable, BorrowedMappedPages, Mutable,
PhysicalAddress, PteFlags, allocate_pages, allocate_frames_at};

use static_assertions::const_assert_eq;
use bitflags::bitflags;
use zerocopy::FromBytes;

/// Physical addresses of the CPU and Distributor
/// interfaces as exposed by the qemu "virt" VM.
pub mod qemu_virt_addrs {
    use super::*;

    pub const GICD: PhysicalAddress = PhysicalAddress::new_canonical(0x08000000);
    pub const GICC: PhysicalAddress = PhysicalAddress::new_canonical(0x08010000);
}

/// The Distributor interface is represented
/// as a series of 4-bytes registers.
#[derive(FromBytes)]
#[repr(C)]
pub struct DistributorInner([u32; 0x4000]);

/// The CPU interface is represented
/// as a series of 4-bytes registers.
#[derive(FromBytes)]
#[repr(C)]
pub struct ProcessorInner([u32; 0x4000]);

const_assert_eq!(core::mem::size_of::<  ProcessorInner>(), 0x10000);
const_assert_eq!(core::mem::size_of::<DistributorInner>(), 0x10000);

/// Arm Generic Interrupt Controller
///
/// The GIC is an extension to ARMv8 which
/// allows routing and filtering interrupts
/// in a single or multi-core system.
pub struct ArmGic {
    distributor: BorrowedMappedPages<DistributorInner, Mutable>,
    processor: BorrowedMappedPages<ProcessorInner, Mutable>,
}

/// Boolean
pub type Enabled = bool;

/// 8-bit unsigned integer
pub type IntNumber = u8;

/// 8-bit unsigned integer
pub type Priority = u8;

bitflags! {
    /// Which CPU to route an interrupt to
    pub struct TargetCpu: u8 {
        const CPU_0 = 1 << 0;
        const CPU_1 = 1 << 1;
        const CPU_2 = 1 << 2;
        const CPU_3 = 1 << 3;
        const CPU_4 = 1 << 4;
        const CPU_5 = 1 << 5;
        const CPU_6 = 1 << 6;
        const CPU_7 = 1 << 7;
        const ALL_CPUS = u8::MAX;
    }
}

// = 4
const U32BYTES: usize = core::mem::size_of::<u32>();

// = 32
const U32BITS: usize = U32BYTES * 8;

// Offsets defined by the GIC specification:

const GICC_CTLR: usize = 0x00 / U32BYTES;
const GICC_PMR:  usize = 0x04 / U32BYTES;
const GICC_IAR:  usize = 0x0C / U32BYTES;
const GICC_RPR:  usize = 0x14 / U32BYTES;
const GICC_EOIR: usize = 0x10 / U32BYTES;

const GICD_CTLR:      usize = 0x000 / U32BYTES;
const GICD_ISENABLER: usize = 0x100 / U32BYTES;
const GICD_ICENABLER: usize = 0x180 / U32BYTES;
const GICD_ITARGETSR: usize = 0x800 / U32BYTES;

impl ArmGic {
    /// Creates the structure
    /// 
    /// Note: this constructor does not access
    /// the interfaces.
    pub const fn new(
        distributor: BorrowedMappedPages<DistributorInner, Mutable>,
        processor: BorrowedMappedPages<ProcessorInner, Mutable>,
    ) -> Self {
        Self {
            distributor,
            processor,
        }
    }

    /// Constructor which maps the two interfaces
    /// based on the passed addresses, using a
    /// page table.
    ///
    /// The mapping process can fail, in which case
    /// this results in an Err(error_message)
    pub fn map(
        page_table: &mut PageTable,
        gicd: PhysicalAddress,
        gicc: PhysicalAddress,
    ) -> Result<Self, &'static str> {
        let num_pages = 16;
        let mmio_flags = PteFlags::DEVICE_MEMORY
                       | PteFlags::NOT_EXECUTABLE
                       | PteFlags::WRITABLE;

        let gicc = {
            let pages = allocate_pages(num_pages).ok_or("couldn't allocate pages for GICC interface")?;
            let frames = allocate_frames_at(gicc, num_pages)?;
            let mapped = page_table.map_allocated_pages_to(pages, frames, mmio_flags)?;
            mapped.into_borrowed_mut(0).map_err(|(_, e)| e)?
        };

        let gicd = {
            let pages = allocate_pages(num_pages).ok_or("couldn't allocate pages for GICD interface")?;
            let frames = allocate_frames_at(gicd, num_pages)?;
            let mapped = page_table.map_allocated_pages_to(pages, frames, mmio_flags)?;
            mapped.into_borrowed_mut(0).map_err(|(_, e)| e)?
        };

        Ok(Self::new(gicd, gicc))
    }

    fn read_gicc(&self, offset: usize) -> u32 {
        self.processor.0[offset]
    }

    fn write_gicc(&mut self, offset: usize, value: u32) {
        self.processor.0[offset] = value;
    }

    fn read_gicd(&self, offset: usize) -> u32 {
        self.distributor.0[offset]
    }

    fn write_gicd(&mut self, offset: usize, value: u32) {
        self.distributor.0[offset] = value;
    }

    // Reads one slot of an array spanning across
    // multiple u32s.
    //
    // - `int` is the index
    // - `offset` tells the beginning of the array
    // - `INTS_PER_U32` = how many array slots per u32 in this array
    fn read_gicd_array<const INTS_PER_U32: usize>(&self, offset: usize, int: IntNumber) -> u32 {
        let int = int as usize;
        let bits_per_int: usize = U32BITS / INTS_PER_U32;
        let mask: u32 = u32::MAX >> (U32BITS - bits_per_int);

        let offset = offset + (int / INTS_PER_U32);
        let reg_index = int & (INTS_PER_U32 - 1);
        let shift = reg_index * bits_per_int;

        let reg = self.read_gicd(offset);
        (reg >> shift) & mask
    }

    // Writes one slot of an array spanning across
    // multiple u32s.
    //
    // - `int` is the index
    // - `offset` tells the beginning of the array
    // - `INTS_PER_U32` = how many array slots per u32 in this array
    // - `value` is the value to write
    fn write_gicd_array<const INTS_PER_U32: usize>(&mut self, offset: usize, int: IntNumber, value: u32) {
        let int = int as usize;
        let bits_per_int: usize = U32BITS / INTS_PER_U32;
        let mask: u32 = u32::MAX >> (U32BITS - bits_per_int);

        let offset = offset + (int / INTS_PER_U32);
        let reg_index = int & (INTS_PER_U32 - 1);
        let shift = reg_index * bits_per_int;

        let mut reg = self.read_gicd(offset);
        reg &= !(mask << shift);
        reg |= (value & mask) << shift;
        self.write_gicd(offset, reg);
    }

    /// Interrupts have a priority; if their priority
    /// is lower or equal to this one, they're discarded
    pub fn get_minimum_int_priority(&self) -> Priority {
        255 - (self.read_gicc(GICC_PMR) as u8)
    }

    /// Interrupts have a priority; if their priority
    /// is lower or equal to this one, they're discarded
    pub fn set_minimum_int_priority(&mut self, priority: Priority) {
        self.write_gicc(GICC_PMR, (255 - priority) as u32)
    }

    /// Is the distributor enabled or disabled?
    ///
    /// When it's disabled, interrupts are not forwarded.
    pub fn get_gicd_state(&self) -> Enabled {
        (self.read_gicd(GICD_CTLR) & 1) > 0
    }

    /// Enables or disables interrupt forwarding in the distributor
    pub fn set_gicd_state(&mut self, enabled: Enabled) {
        let mut reg = self.read_gicc(GICD_CTLR);
        reg &= !1;
        reg |= enabled as u32;
        self.write_gicd(GICD_CTLR, reg);
    }

    /// Is the cpu interface enabled or disabled?
    ///
    /// When it's disabled, interrupts are not forwarded.
    pub fn get_gicc_state(&self) -> Enabled {
        (self.read_gicc(GICC_CTLR) & 1) > 0
    }

    /// Enables or disables interrupt forwarding in the cpu interface
    pub fn set_gicc_state(&mut self, enabled: Enabled) {
        let mut reg = self.read_gicc(GICD_CTLR);
        reg &= !1;
        reg |= enabled as u32;
        self.write_gicc(GICC_CTLR, reg);
    }

    /// Will that interrupt be forwarded by the GIC?
    pub fn get_int_state(&self, int: IntNumber) -> Enabled {
        self.read_gicd_array::<32>(GICD_ISENABLER, int) > 0
    }

    /// Enables or disables the forwarding of
    /// a particular interrupt
    pub fn set_int_state(&mut self, int: IntNumber, enabled: Enabled) {
        let reg_base = match enabled {
            true => GICD_ISENABLER,
            false => GICD_ICENABLER,
        };
        self.write_gicd_array::<32>(reg_base, int, 1);
    }

    /// Which CPU cores will the GIC forward
    /// that interrupt to?
    pub fn get_int_target(&self, int: IntNumber) -> TargetCpu {
        let flags = self.read_gicd_array::<4>(GICD_ITARGETSR, int);
        TargetCpu::from_bits_truncate(flags as u8)
    }

    /// Sets which CPU cores to forward that
    /// interrupt to, when it's received
    pub fn set_int_target(&mut self, int: IntNumber, target: TargetCpu) {
        self.write_gicd_array::<4>(GICD_ITARGETSR, int, target.bits as u32);
    }

    /// Performs priority drop for the specified interrupt
    pub fn end_of_interrupt(&mut self, int: IntNumber) {
        self.write_gicc(GICC_EOIR, int as u32);
    }

    /// Acknowledge the currently serviced interrupt
    /// and fetches its number
    /// 
    /// Note: this constructor accesses the
    /// interfaces; their addresses have to be
    /// readable and writable.
    pub fn acknowledge_int(&mut self) -> (IntNumber, Priority) {
        // Reading the interrupt number has the side effect
        // of acknowledging the interrupt.
        let int_num = self.read_gicc(GICC_IAR) as u8;
        let priority = self.read_gicc(GICC_RPR) as u8;

        (int_num, priority)
    }
}
