use core::convert::AsMut;

use memory::{PageTable, BorrowedMappedPages, Mutable,
PhysicalAddress, PteFlags, allocate_pages, allocate_frames_at};

use static_assertions::const_assert_eq;
use bitflags::bitflags;

mod cpu_interface_gicv3;
mod cpu_interface_gicv2;
mod dist_interface;
mod redist_interface;

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

/// An Interrupt Number
pub type InterruptNumber = u32;

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

#[repr(C)]
#[derive(zerocopy::FromBytes)]
pub struct GicMappedPage {
    inner: [u32; 0x400],
}

impl GicMappedPage {
    fn read_volatile(&self, index: usize) -> u32 {
        unsafe { (&self.inner[index] as *const u32).read_volatile() }
    }

    fn write_volatile(&mut self, index: usize, value: u32) {
        unsafe { (&mut self.inner[index] as *mut u32).write_volatile(value) }
    }

    // Reads one slot of an array spanning across
    // multiple u32s.
    //
    // - `int` is the index
    // - `offset` tells the beginning of the array
    // - `INTS_PER_U32` = how many array slots per u32 in this array
    fn read_array_volatile<const INTS_PER_U32: usize>(&self, offset: usize, int: InterruptNumber) -> u32 {
        let int = int as usize;
        let bits_per_int: usize = U32BITS / INTS_PER_U32;
        let mask: u32 = u32::MAX >> (U32BITS - bits_per_int);

        let offset = offset + (int / INTS_PER_U32);
        let reg_index = int & (INTS_PER_U32 - 1);
        let shift = reg_index * bits_per_int;

        let reg = self.read_volatile(offset);
        (reg >> shift) & mask
    }

    // Writes one slot of an array spanning across
    // multiple u32s.
    //
    // - `int` is the index
    // - `offset` tells the beginning of the array
    // - `INTS_PER_U32` = how many array slots per u32 in this array
    // - `value` is the value to write
    fn write_array_volatile<const INTS_PER_U32: usize>(&mut self, offset: usize, int: InterruptNumber, value: u32) {
        let int = int as usize;
        let bits_per_int: usize = U32BITS / INTS_PER_U32;
        let mask: u32 = u32::MAX >> (U32BITS - bits_per_int);

        let offset = offset + (int / INTS_PER_U32);
        let reg_index = int & (INTS_PER_U32 - 1);
        let shift = reg_index * bits_per_int;

        let mut reg = self.read_volatile(offset);
        reg &= !(mask << shift);
        reg |= (value & mask) << shift;
        self.write_volatile(offset, reg);
    }
}

const_assert_eq!(core::mem::size_of::<GicMappedPage>(), 0x1000);

const REDIST_SGIPPI_OFFSET: usize = 0x10000;
const DIST_P6_OFFSET: usize = 0x6000;

pub struct ArmGicV2 {
    pub distributor: BorrowedMappedPages<GicMappedPage, Mutable>,
    pub processor: BorrowedMappedPages<GicMappedPage, Mutable>,
}

pub struct ArmGicV3 {
    pub affinity_routing: Enabled,
    pub distributor: BorrowedMappedPages<GicMappedPage, Mutable>,
    pub dist_extended: BorrowedMappedPages<GicMappedPage, Mutable>,
    pub redistributor: BorrowedMappedPages<GicMappedPage, Mutable>,
    pub redist_sgippi: BorrowedMappedPages<GicMappedPage, Mutable>,
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

        let mut map_dist = |gicd_base| -> Result<BorrowedMappedPages<GicMappedPage, Mutable>, &'static str>  {
            let pages = allocate_pages(1).ok_or("couldn't allocate pages for the distributor interface")?;
            let frames = allocate_frames_at(gicd_base, 1)?;
            let mapped = page_table.map_allocated_pages_to(pages, frames, mmio_flags)?;
            mapped.into_borrowed_mut(0).map_err(|(_, e)| e)
        };

        match version {
            Version::InitV2 { dist, cpu } => {
                let mut distributor = map_dist(dist)?;

                let mut processor: BorrowedMappedPages<GicMappedPage, Mutable> = {
                    let pages = allocate_pages(1).ok_or("couldn't allocate pages for the CPU interface")?;
                    let frames = allocate_frames_at(cpu, 1)?;
                    let mapped = page_table.map_allocated_pages_to(pages, frames, mmio_flags)?;
                    mapped.into_borrowed_mut(0).map_err(|(_, e)| e)?
                };

                cpu_interface_gicv2::init(processor.as_mut());
                dist_interface::init(distributor.as_mut());

                Ok(Self::V2(ArmGicV2 { distributor, processor }))
            },
            Version::InitV3 { dist, redist } => {
                let mut distributor = map_dist(dist)?;

                let dist_extended: BorrowedMappedPages<GicMappedPage, Mutable> = {
                    let pages = allocate_pages(1).ok_or("couldn't allocate pages for the extended distributor interface")?;
                    let frames = allocate_frames_at(dist + DIST_P6_OFFSET, 1)?;
                    let mapped = page_table.map_allocated_pages_to(pages, frames, mmio_flags)?;
                    mapped.into_borrowed_mut(0).map_err(|(_, e)| e)?
                };

                let mut redistributor: BorrowedMappedPages<GicMappedPage, Mutable> = {
                    let pages = allocate_pages(1).ok_or("couldn't allocate pages for the redistributor interface")?;
                    let frames = allocate_frames_at(redist, 1)?;
                    let mapped = page_table.map_allocated_pages_to(pages, frames, mmio_flags)?;
                    mapped.into_borrowed_mut(0).map_err(|(_, e)| e)?
                };

                let redist_sgippi = {
                    let pages = allocate_pages(1).ok_or("couldn't allocate pages for the extended redistributor interface")?;
                    let frames = allocate_frames_at(redist + REDIST_SGIPPI_OFFSET, 1)?;
                    let mapped = page_table.map_allocated_pages_to(pages, frames, mmio_flags)?;
                    mapped.into_borrowed_mut(0).map_err(|(_, e)| e)?
                };

                redist_interface::init(redistributor.as_mut())?;
                cpu_interface_gicv3::init();
                let affinity_routing = dist_interface::init(distributor.as_mut());

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
    pub fn send_ipi(&mut self, int_num: InterruptNumber, target: TargetCpu) {
        assert!(int_num < 16, "IPIs must have a number below 16 on ARMv8");

        match self {
            Self::V2(v2) => dist_interface::send_ipi_gicv2(&mut v2.distributor, int_num, target),
            Self::V3( _) => cpu_interface_gicv3::send_ipi(int_num, target),
        }
    }

    /// Acknowledge the currently serviced interrupt
    /// and fetches its number
    pub fn acknowledge_interrupt(&mut self) -> (InterruptNumber, Priority) {
        match self {
            Self::V2(v2) => cpu_interface_gicv2::acknowledge_interrupt(&mut v2.processor),
            Self::V3( _) => cpu_interface_gicv3::acknowledge_interrupt(),
        }
    }

    /// Performs priority drop for the specified interrupt
    pub fn end_of_interrupt(&mut self, int: InterruptNumber) {
        match self {
            Self::V2(v2) => cpu_interface_gicv2::end_of_interrupt(&mut v2.processor, int),
            Self::V3( _) => cpu_interface_gicv3::end_of_interrupt(int),
        }
    }

    /// Will that interrupt be forwarded by the distributor?
    pub fn get_interrupt_state(&self, int: InterruptNumber) -> Enabled {
        match int {
            0..=31 => if let Self::V3(v3) = self {
                redist_interface::get_sgippi_state(&v3.redist_sgippi, int)
            } else {
                true
            },
            _ => dist_interface::get_spi_state(self.distributor(), int),
        }
    }

    /// Enables or disables the forwarding of
    /// a particular interrupt in the distributor
    pub fn set_interrupt_state(&mut self, int: InterruptNumber, enabled: Enabled) {
        match int {
            0..=31 => if let Self::V3(v3) = self {
                redist_interface::set_sgippi_state(&mut v3.redist_sgippi, int, enabled);
            },
            _ => dist_interface::set_spi_state(self.distributor_mut(), int, enabled),
        };
    }

    /// Interrupts have a priority; if their priority
    /// is lower or equal to this one, they're queued
    /// until this CPU or another one is ready to handle
    /// them
    pub fn get_minimum_priority(&self) -> Priority {
        match self {
            Self::V2(v2) => cpu_interface_gicv2::get_minimum_priority(&v2.processor),
            Self::V3( _) => cpu_interface_gicv3::get_minimum_priority(),
        }
    }

    /// Interrupts have a priority; if their priority
    /// is lower or equal to this one, they're queued
    /// until this CPU or another one is ready to handle
    /// them
    pub fn set_minimum_priority(&mut self, priority: Priority) {
        match self {
            Self::V2(v2) => cpu_interface_gicv2::set_minimum_priority(&mut v2.processor, priority),
            Self::V3( _) => cpu_interface_gicv3::set_minimum_priority(priority),
        }
    }
}
