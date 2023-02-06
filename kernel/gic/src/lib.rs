//! Allows configuring the Generic Interrupt Controller
#![no_std]

use core::convert::AsMut;

use memory::{PageTable, BorrowedMappedPages, Mutable,
PhysicalAddress, PteFlags, allocate_pages, allocate_frames_at};

use static_assertions::const_assert_eq;
use bitflags::bitflags;

/// Physical addresses of the CPU and Distributor
/// interfaces as exposed by the qemu "virt" VM.
pub mod qemu_virt_addrs {
    use super::*;

    pub const GICD: PhysicalAddress = PhysicalAddress::new_canonical(0x08000000);
    pub const GICC: PhysicalAddress = PhysicalAddress::new_canonical(0x08010000);
    pub const GICR: PhysicalAddress = PhysicalAddress::new_canonical(0x080A0000);
}

/// Boolean
pub type Enabled = bool;

/// 24-bit unsigned integer
///
/// An u32 is used because there is no u24.
pub type IntNumber = u32;

/// 8-bit unsigned integer
pub type Priority = u8;

bitflags! {
    /// The legacy (GICv2) way of specifying
    /// multiple target CPUs
    pub struct TargetList: u8 {
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

pub enum TargetCpu {
    /// That interrupt must be handled by
    /// a specific PE in the system
    Specific(u32),
    /// That interrupt can be handled by
    /// any PE that is not busy with another,
    /// more important task
    AnyCpuAvailable,
    GICv2TargetList(TargetList),
}

// = 4
const U32BYTES: usize = core::mem::size_of::<u32>();
// = 32
const U32BITS: usize = U32BYTES * 8;

pub type MmioPageOfU32 = [u32; 0x400];

type BorrowedPage = BorrowedMappedPages<MmioPageOfU32, Mutable>;

const REDIST_SGIPPI_OFFSET: usize = 0x10000;
const DIST_P6_OFFSET: usize = 0x6000;

const_assert_eq!(core::mem::size_of::<MmioPageOfU32>(), 0x1000);

mod cpu3;
mod cpu2;
mod dist;
mod redist;

// Reads one slot of an array spanning across
// multiple u32s.
//
// - `int` is the index
// - `offset` tells the beginning of the array
// - `INTS_PER_U32` = how many array slots per u32 in this array
fn read_array<const INTS_PER_U32: usize>(array: &[u32], offset: usize, int: IntNumber) -> u32 {
    let int = int as usize;
    let bits_per_int: usize = U32BITS / INTS_PER_U32;
    let mask: u32 = u32::MAX >> (U32BITS - bits_per_int);

    let offset = offset + (int / INTS_PER_U32);
    let reg_index = int & (INTS_PER_U32 - 1);
    let shift = reg_index * bits_per_int;

    let reg = array[offset];
    (reg >> shift) & mask
}

// Writes one slot of an array spanning across
// multiple u32s.
//
// - `int` is the index
// - `offset` tells the beginning of the array
// - `INTS_PER_U32` = how many array slots per u32 in this array
// - `value` is the value to write
fn write_array<const INTS_PER_U32: usize>(array: &mut [u32], offset: usize, int: IntNumber, value: u32) {
    let int = int as usize;
    let bits_per_int: usize = U32BITS / INTS_PER_U32;
    let mask: u32 = u32::MAX >> (U32BITS - bits_per_int);

    let offset = offset + (int / INTS_PER_U32);
    let reg_index = int & (INTS_PER_U32 - 1);
    let shift = reg_index * bits_per_int;

    let mut reg = array[offset];
    reg &= !(mask << shift);
    reg |= (value & mask) << shift;
    array[offset] = reg;
}

pub struct ArmGicV2 {
    pub distributor: BorrowedPage,
    pub processor: BorrowedPage,
}

pub struct ArmGicV3 {
    pub affinity_routing: Enabled,
    pub distributor: BorrowedPage,
    pub dist_extended: BorrowedPage,
    pub redistributor: BorrowedPage,
    pub redist_sgippi: BorrowedPage,
}

/// Arm Generic Interrupt Controller
///
/// The GIC is an extension to ARMv8 which
/// allows routing and filtering interrupts
/// in a single or multi-core system.
pub enum ArmGic {
    V2(ArmGicV2),
    V3(ArmGicV3),
}

pub enum Version {
    InitV2 {
        dist: PhysicalAddress,
        cpu: PhysicalAddress,
    },
    InitV3 {
        dist: PhysicalAddress,
        redist: PhysicalAddress,
    }
}

impl ArmGic {
    pub fn init(page_table: &mut PageTable, version: Version) -> Result<Self, &'static str> {
        let mmio_flags = PteFlags::DEVICE_MEMORY
                       | PteFlags::NOT_EXECUTABLE
                       | PteFlags::WRITABLE;

        let mut map_dist = |gicd_base| -> Result<BorrowedPage, &'static str>  {
            let pages = allocate_pages(1).ok_or("couldn't allocate pages for GICC interface")?;
            let frames = allocate_frames_at(gicd_base, 1)?;
            let mapped = page_table.map_allocated_pages_to(pages, frames, mmio_flags)?;
            mapped.into_borrowed_mut(0).map_err(|(_, e)| e)
        };

        match version {
            Version::InitV2 { dist, cpu } => {
                let mut distributor = map_dist(dist)?;

                let mut processor: BorrowedPage = {
                    let pages = allocate_pages(1).ok_or("couldn't allocate pages for GICC interface")?;
                    let frames = allocate_frames_at(cpu, 1)?;
                    let mapped = page_table.map_allocated_pages_to(pages, frames, mmio_flags)?;
                    mapped.into_borrowed_mut(0).map_err(|(_, e)| e)?
                };

                cpu2::init(processor.as_mut());
                dist::init(distributor.as_mut());

                Ok(Self::V2(ArmGicV2 { distributor, processor }))
            },
            Version::InitV3 { dist, redist } => {
                let mut distributor = map_dist(dist)?;

                let dist_extended: BorrowedPage = {
                    let pages = allocate_pages(1).ok_or("couldn't allocate pages for GICC interface")?;
                    let frames = allocate_frames_at(dist + DIST_P6_OFFSET, 1)?;
                    let mapped = page_table.map_allocated_pages_to(pages, frames, mmio_flags)?;
                    mapped.into_borrowed_mut(0).map_err(|(_, e)| e)?
                };

                let mut redistributor: BorrowedPage = {
                    let pages = allocate_pages(1).ok_or("couldn't allocate pages for GICC interface")?;
                    let frames = allocate_frames_at(redist, 1)?;
                    let mapped = page_table.map_allocated_pages_to(pages, frames, mmio_flags)?;
                    mapped.into_borrowed_mut(0).map_err(|(_, e)| e)?
                };

                let redist_sgippi = {
                    let pages = allocate_pages(1).ok_or("couldn't allocate pages for GICC interface")?;
                    let frames = allocate_frames_at(redist + REDIST_SGIPPI_OFFSET, 1)?;
                    let mapped = page_table.map_allocated_pages_to(pages, frames, mmio_flags)?;
                    mapped.into_borrowed_mut(0).map_err(|(_, e)| e)?
                };

                redist::init(redistributor.as_mut());
                cpu3::init();
                let affinity_routing = dist::init(distributor.as_mut());

                Ok(Self::V3(ArmGicV3 { distributor, dist_extended, redistributor, redist_sgippi, affinity_routing }))
            },
        }
    }

    fn affinity_routing(&self) -> Enabled {
        match self {
            Self::V2( _) => false,
            Self::V3(v3) => v3.affinity_routing,
        }
    }

    /// Sends an inter processor interrupt (IPI),
    /// also called software generated interrupt (SGI).
    ///
    /// note: on Aarch64, IPIs must have a number below 16 on ARMv8
    pub fn send_ipi(&mut self, int_num: IntNumber, target: TargetCpu) {
        assert!(int_num < 16, "IPIs must have a number below 16 on ARMv8");

        match self {
            Self::V2(v2) => dist::send_ipi_gicv2(&mut v2.distributor, int_num, target),
            Self::V3( _) => cpu3::send_ipi(int_num, target),
        }
    }

    /// Acknowledge the currently serviced interrupt
    /// and fetches its number
    /// 
    /// Note: this constructor accesses the
    /// interfaces; their addresses have to be
    /// readable and writable.
    pub fn acknowledge_int(&mut self) -> (IntNumber, Priority) {
        match self {
            Self::V2(v2) => cpu2::acknowledge_int(&mut v2.processor),
            Self::V3( _) => cpu3::acknowledge_int(),
        }
    }

    /// Performs priority drop for the specified interrupt
    pub fn end_of_interrupt(&mut self, int: IntNumber) {
        match self {
            Self::V2(v2) => cpu2::end_of_interrupt(&mut v2.processor, int),
            Self::V3( _) => cpu3::end_of_interrupt(int),
        }
    }

    /// Will that interrupt be forwarded by the distributor?
    pub fn get_int_state(&self, int: IntNumber) -> Enabled {
        match int {
            0..=31 => if let Self::V3(v3) = self {
                redist::get_sgippi_state(&v3.redist_sgippi, int)
            } else {
                true
            },
            _ => dist::get_int_state(self.distributor(), int),
        }
    }

    /// Enables or disables the forwarding of
    /// a particular interrupt in the distributor
    pub fn set_int_state(&mut self, int: IntNumber, enabled: Enabled) {
        match int {
            0..=31 => if let Self::V3(v3) = self {
                redist::set_sgippi_state(&mut v3.redist_sgippi, int, enabled);
            },
            _ => dist::set_int_state(self.distributor_mut(), int, enabled),
        };
    }

    /// Interrupts have a priority; if their priority
    /// is lower or equal to this one, they're discarded
    pub fn get_minimum_int_priority(&self) -> Priority {
        match self {
            Self::V2(v2) => cpu2::get_minimum_int_priority(&v2.processor),
            Self::V3( _) => cpu3::get_minimum_int_priority(),
        }
    }

    /// Interrupts have a priority; if their priority
    /// is lower or equal to this one, they're discarded
    pub fn set_minimum_int_priority(&mut self, priority: Priority) {
        match self {
            Self::V2(v2) => cpu2::set_minimum_int_priority(&mut v2.processor, priority),
            Self::V3( _) => cpu3::set_minimum_int_priority(priority),
        }
    }
}
