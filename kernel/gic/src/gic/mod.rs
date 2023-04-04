use core::convert::AsMut;

use memory::{PageTable, BorrowedMappedPages, Mutable,
PhysicalAddress, PteFlags, allocate_pages, allocate_frames_at};

use static_assertions::const_assert_eq;
use bitflags::bitflags;

mod cpu_interface_gicv3;
mod cpu_interface_gicv2;
mod dist_interface;
mod redist_interface;

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
    /// a specific PE in the system.
    ///
    /// - level 3 affinity is expected in bits [24:31]
    /// - level 2 affinity is expected in bits [16:23]
    /// - level 1 affinity is expected in bits [8:15]
    /// - level 0 affinity is expected in bits [0:7]
    Specific(u32),
    /// That interrupt can be handled by
    /// any PE that is not busy with another,
    /// more important task
    AnyCpuAvailable,
    GICv2TargetList(TargetList),
}

const U32BITS: usize = u32::BITS as usize;

#[derive(Copy, Clone)]
pub(crate) struct Offset32(usize);

#[derive(Copy, Clone)]
pub(crate) struct Offset64(usize);

impl Offset32 {
    pub(crate) const fn from_byte_offset(byte_offset: usize) -> Self {
        Self(byte_offset / core::mem::size_of::<u32>())
    }
}

impl Offset64 {
    pub(crate) const fn from_byte_offset(byte_offset: usize) -> Self {
        Self(byte_offset / core::mem::size_of::<u64>())
    }
}

#[repr(C)]
#[derive(zerocopy::FromBytes)]
pub struct GicRegisters {
    inner: [u32; 0x400],
}

impl GicRegisters {
    fn read_volatile(&self, offset: Offset32) -> u32 {
        unsafe { (&self.inner[offset.0] as *const u32).read_volatile() }
    }

    fn write_volatile(&mut self, offset: Offset32, value: u32) {
        unsafe { (&mut self.inner[offset.0] as *mut u32).write_volatile(value) }
    }

    fn read_volatile_64(&self, offset: Offset64) -> u64 {
        unsafe { (self.inner.as_ptr() as *const u64).add(offset.0).read_volatile() }
    }

    fn write_volatile_64(&mut self, offset: Offset64, value: u64) {
        unsafe { (self.inner.as_mut_ptr() as *mut u64).add(offset.0).write_volatile(value) }
    }

    // Reads one item of an array spanning across
    // multiple u32s.
    //
    // The maximum item size is 32 bits, and the items are always aligned to 2**N bits.
    // The array spans multiple adjacent u32s but there is always a integer number of
    // items in a single u32.
    //
    // - `int` is the index
    // - `offset` tells the beginning of the array
    // - `INTS_PER_U32` = how many array slots per u32 in this array
    fn read_array_volatile<const INTS_PER_U32: usize>(&self, offset: Offset32, int: InterruptNumber) -> u32 {
        let int = int as usize;
        let bits_per_int: usize = U32BITS / INTS_PER_U32;
        let mask: u32 = u32::MAX >> (U32BITS - bits_per_int);

        let offset = Offset32(offset.0 + (int / INTS_PER_U32));
        let reg_index = int & (INTS_PER_U32 - 1);
        let shift = reg_index * bits_per_int;

        let reg = self.read_volatile(offset);
        (reg >> shift) & mask
    }

    // Writes one item of an array spanning across
    // multiple u32s.
    //
    // The maximum item size is 32 bits, and the items are always aligned to 2**N bits.
    // The array spans multiple adjacent u32s but there is always a integer number of
    // items in a single u32.
    //
    // - `int` is the index
    // - `offset` tells the beginning of the array
    // - `INTS_PER_U32` = how many array slots per u32 in this array
    // - `value` is the value to write
    fn write_array_volatile<const INTS_PER_U32: usize>(&mut self, offset: Offset32, int: InterruptNumber, value: u32) {
        let int = int as usize;
        let bits_per_int: usize = U32BITS / INTS_PER_U32;
        let mask: u32 = u32::MAX >> (U32BITS - bits_per_int);

        let offset = Offset32(offset.0 + (int / INTS_PER_U32));
        let reg_index = int & (INTS_PER_U32 - 1);
        let shift = reg_index * bits_per_int;

        let mut reg = self.read_volatile(offset);
        reg &= !(mask << shift);
        reg |= (value & mask) << shift;
        self.write_volatile(offset, reg);
    }
}

const_assert_eq!(core::mem::size_of::<GicRegisters>(), 0x1000);

// Returns the index to the redistributor base address of this
// CPU in the array of register base addresses, which is defined
// in `arm_boards::INTERRUPT_CONTROLLER_CONFIG`.
fn get_current_cpu_redist_index() -> usize {
    let cpu_id = cpu::current_cpu();
    arm_boards::CPUIDS.iter()
          .position(|id| *id == cpu_id)
          .expect("BUG: get_current_cpu_redist_index: unexpected CpuId for current CPU")
}

const REDIST_SGIPPI_OFFSET: usize = 0x10000;
const DIST_P6_OFFSET: usize = 0x6000;

pub struct ArmGicV2 {
    pub distributor: BorrowedMappedPages<GicRegisters, Mutable>,
    pub processor: BorrowedMappedPages<GicRegisters, Mutable>,
}

pub struct ArmGicV3RedistPages {
    pub redistributor: BorrowedMappedPages<GicRegisters, Mutable>,
    pub redist_sgippi: BorrowedMappedPages<GicRegisters, Mutable>,
}

pub struct ArmGicV3 {
    pub affinity_routing: Enabled,
    pub distributor: BorrowedMappedPages<GicRegisters, Mutable>,
    pub dist_extended: BorrowedMappedPages<GicRegisters, Mutable>,
    pub redistributors: [ArmGicV3RedistPages; arm_boards::CPUS],
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
        redist: [PhysicalAddress; arm_boards::CPUS],
    }
}

impl ArmGic {
    pub fn init(page_table: &mut PageTable, version: Version) -> Result<Self, &'static str> {
        let mmio_flags = PteFlags::DEVICE_MEMORY
                       | PteFlags::NOT_EXECUTABLE
                       | PteFlags::WRITABLE;

        let mut map_dist = |gicd_base| -> Result<BorrowedMappedPages<GicRegisters, Mutable>, &'static str>  {
            let pages = allocate_pages(1).ok_or("couldn't allocate pages for the distributor interface")?;
            let frames = allocate_frames_at(gicd_base, 1)?;
            let mapped = page_table.map_allocated_pages_to(pages, frames, mmio_flags)?;
            mapped.into_borrowed_mut(0).map_err(|(_, e)| e)
        };

        match version {
            Version::InitV2 { dist, cpu } => {
                let mut distributor = map_dist(dist)?;

                let mut processor: BorrowedMappedPages<GicRegisters, Mutable> = {
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

                let dist_extended: BorrowedMappedPages<GicRegisters, Mutable> = {
                    let pages = allocate_pages(1).ok_or("couldn't allocate pages for the extended distributor interface")?;
                    let frames = allocate_frames_at(dist + DIST_P6_OFFSET, 1)?;
                    let mapped = page_table.map_allocated_pages_to(pages, frames, mmio_flags)?;
                    mapped.into_borrowed_mut(0).map_err(|(_, e)| e)?
                };

                let redistributors: [ArmGicV3RedistPages; arm_boards::CPUS] = core::array::try_from_fn(|i| {
                    let phys_addr = redist[i];

                    let mut redistributor: BorrowedMappedPages<GicRegisters, Mutable> = {
                        let pages = allocate_pages(1).ok_or("couldn't allocate pages for the redistributor interface")?;
                        let frames = allocate_frames_at(phys_addr, 1)?;
                        let mapped = page_table.map_allocated_pages_to(pages, frames, mmio_flags)?;
                        mapped.into_borrowed_mut(0).map_err(|(_, e)| e)?
                    };

                    redist_interface::init(redistributor.as_mut())?;

                    let redist_sgippi = {
                        let pages = allocate_pages(1).ok_or("couldn't allocate pages for the extended redistributor interface")?;
                        let frames = allocate_frames_at(phys_addr + REDIST_SGIPPI_OFFSET, 1)?;
                        let mapped = page_table.map_allocated_pages_to(pages, frames, mmio_flags)?;
                        mapped.into_borrowed_mut(0).map_err(|(_, e)| e)?
                    };

                    Ok::<ArmGicV3RedistPages, &'static str>(ArmGicV3RedistPages {
                        redistributor,
                        redist_sgippi,
                    })
                })?;

                // this cannot fail as we pushed exactly `arm_boards::CPUS` items
                // let redistributors = redistributors.into_inner().unwrap();

                cpu_interface_gicv3::init();
                let affinity_routing = dist_interface::init(distributor.as_mut());

                Ok(Self::V3(ArmGicV3 { distributor, dist_extended, redistributors, affinity_routing }))
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
                let i = get_current_cpu_redist_index();
                redist_interface::is_sgippi_enabled(&v3.redistributors[i].redist_sgippi, int)
            } else {
                true
            },
            _ => dist_interface::is_spi_enabled(self.distributor(), int),
        }
    }

    /// Enables or disables the forwarding of
    /// a particular interrupt in the distributor
    pub fn set_interrupt_state(&mut self, int: InterruptNumber, enabled: Enabled) {
        match int {
            0..=31 => if let Self::V3(v3) = self {
                let i = get_current_cpu_redist_index();
                redist_interface::enable_sgippi(&mut v3.redistributors[i].redist_sgippi, int, enabled);
            },
            _ => dist_interface::enable_spi(self.distributor_mut(), int, enabled),
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

impl core::fmt::Debug for ArmGicV3RedistPages {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "ArmGicV3RedistPages")
    }
}
