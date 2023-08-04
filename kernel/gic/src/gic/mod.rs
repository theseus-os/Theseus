use core::convert::AsMut;

use cpu::{CpuId, MpidrValue};
use arm_boards::{BOARD_CONFIG, NUM_CPUS};
use memory::{
    BorrowedMappedPages, Mutable, PhysicalAddress,
    MMIO_FLAGS, map_frame_range, PAGE_SIZE,
};

use static_assertions::const_assert_eq;

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

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct TargetList(u8);

impl TargetList {
    pub fn new<T: Iterator<Item=MpidrValue>>(list: T) -> Result<Self, &'static str> {
        let mut this = 0;

        for mpidr in list {
            let mpidr = mpidr.value();
            if mpidr < 8 {
                this |= 1 << mpidr;
            } else {
                return Err("CPU id is too big for GICv2 (should be < 8)");
            }
        }

        Ok(Self(this))
    }

    /// Tries to create a `TargetList` from `arm_boards::BOARD_CONFIG.cpu_ids`
    pub fn new_all_cpus() -> Result<Self, &'static str> {
        let list = BOARD_CONFIG.cpu_ids.iter().map(|def_mpidr| (*def_mpidr).into());
        Self::new(list).map_err(|_| "Some CPUs in the system cannot be stored in a TargetList")
    }

    pub fn iter(self) -> TargetListIterator {
        TargetListIterator {
            bitfield: self.0,
            shift: 0,
        }
    }
}

pub struct TargetListIterator {
    bitfield: u8,
    shift: u8,
}

impl Iterator for TargetListIterator {
    type Item = Result<CpuId, &'static str>;
    fn next(&mut self) -> Option<Self::Item> {
        while ((self.bitfield >> self.shift) & 1 == 0) && self.shift < 8 {
            self.shift += 1;
        }

        if self.shift < 8 {
            let def_mpidr = MpidrValue::try_from(self.shift as u64);
            self.shift += 1;
            Some(def_mpidr.map(|m| m.into()))
        } else {
            None
        }
    }
}

/// Target of a shared-peripheral interrupt
#[derive(Copy, Clone, Debug)]
pub enum SpiDestination {
    /// The interrupt must be delivered to a specific CPU.
    Specific(CpuId),
    /// That interrupt can be handled by any PE that is not busy with another, more
    /// important task
    AnyCpuAvailable,
    /// The interrupt will be delivered to all CPUs specified by the included target list
    GICv2TargetList(TargetList),
}

/// Target of an inter-processor interrupt
#[derive(Copy, Clone, Debug)]
pub enum IpiTargetCpu {
    /// The interrupt will be delivered to a specific CPU.
    Specific(CpuId),
    /// The interrupt will be delivered to all CPUs except the sender.
    AllOtherCpus,
    /// The interrupt will be delivered to all CPUs specified by the included target list
    GICv2TargetList(TargetList),
}

impl SpiDestination {
    /// When this is a GICv2TargetList, converts the list to
    /// `AnyCpuAvailable` if the list contains all CPUs.
    pub fn canonicalize(self) -> Self {
        match self {
            Self::Specific(cpu_id) => Self::Specific(cpu_id),
            Self::AnyCpuAvailable => Self::AnyCpuAvailable,
            Self::GICv2TargetList(list) => match TargetList::new_all_cpus() == Ok(list) {
                true => Self::AnyCpuAvailable,
                false => Self::GICv2TargetList(list),
            },
        }
    }
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

const REDIST_SGIPPI_OFFSET: usize = 0x10000;
const DIST_P6_OFFSET: usize = 0x6000;


pub struct ArmGicV3RedistPages {
    pub redistributor: BorrowedMappedPages<GicRegisters, Mutable>,
    pub redist_sgippi: BorrowedMappedPages<GicRegisters, Mutable>,
}

pub enum Version {
    InitV2 {
        dist: PhysicalAddress,
        cpu: PhysicalAddress,
    },
    InitV3 {
        dist: PhysicalAddress,
        redist: [PhysicalAddress; NUM_CPUS],
    }
}

pub enum ArmGicDistributor {
    V2 {
        registers: BorrowedMappedPages<GicRegisters, Mutable>,
    },
    V3 {
        affinity_routing: Enabled,
        v2_regs: BorrowedMappedPages<GicRegisters, Mutable>,
        v3_regs: BorrowedMappedPages<GicRegisters, Mutable>,
    },
}

impl ArmGicDistributor {
    pub fn init(version: &Version) -> Result<Self, &'static str> {
        match version {
            Version::InitV2 { dist, .. } => {
                let mapped = map_frame_range(*dist, PAGE_SIZE, MMIO_FLAGS)?;
                let mut registers = mapped
                    .into_borrowed_mut::<GicRegisters>(0)
                    .map_err(|(_, e)| e)?;

                dist_interface::init(registers.as_mut());

                Ok(Self::V2 {
                    registers,
                })
            },
            Version::InitV3 { dist, .. } => {
                let mapped = map_frame_range(*dist, PAGE_SIZE, MMIO_FLAGS)?;
                let mut v2_regs = mapped
                    .into_borrowed_mut::<GicRegisters>(0)
                    .map_err(|(_, e)| e)?;

                let v3_regs: BorrowedMappedPages<GicRegisters, Mutable> = {
                    let paddr = *dist + DIST_P6_OFFSET;
                    let mapped = map_frame_range(paddr, PAGE_SIZE, MMIO_FLAGS)?;
                    mapped.into_borrowed_mut(0).map_err(|(_, e)| e)?
                };

                let affinity_routing = dist_interface::init(v2_regs.as_mut());

                Ok(Self::V3 {
                    affinity_routing,
                    v2_regs,
                    v3_regs,
                })
            },
        }
    }

    /// Will that interrupt be forwarded by the distributor?
    pub fn get_spi_state(&self, int: InterruptNumber) -> Enabled {
        assert!(int >= 32);
        dist_interface::is_spi_enabled(self.distributor(), int)
    }

    /// Enables or disables the forwarding of
    /// a particular interrupt in the distributor
    pub fn set_spi_state(&mut self, int: InterruptNumber, enabled: Enabled) {
        assert!(int >= 32);
        dist_interface::enable_spi(self.distributor_mut(), int, enabled)
    }

    /// Returns the priority of an interrupt
    pub fn get_spi_priority(&self, int: InterruptNumber) -> Priority {
        assert!(int >= 32);
        dist_interface::get_spi_priority(self.distributor(), int)
    }

    /// Sets the priority of an interrupt (0-255)
    pub fn set_spi_priority(&mut self, int: InterruptNumber, enabled: Priority) {
        assert!(int >= 32);
        dist_interface::set_spi_priority(self.distributor_mut(), int, enabled)
    }
}

pub enum ArmGicCpuComponents {
    V2 {
        registers: BorrowedMappedPages<GicRegisters, Mutable>,
        cpu_index: u16,
    },
    V3 {
        redist_regs: ArmGicV3RedistPages,
    },
}

impl ArmGicCpuComponents {
    pub fn init(cpu_id: CpuId, version: &Version) -> Result<Self, &'static str> {
        let cpu_index = BOARD_CONFIG.cpu_ids.iter()
            .position(|mpidr| CpuId::from(*mpidr) == cpu_id)
            .expect("BUG: invalid CpuId in ArmGicCpuComponents::init");

        match version {
            Version::InitV2 { cpu, .. } => {
                let mut registers: BorrowedMappedPages<GicRegisters, Mutable> = {
                    let mapped = map_frame_range(*cpu, PAGE_SIZE, MMIO_FLAGS)?;
                    mapped.into_borrowed_mut(0).map_err(|(_, e)| e)?
                };

                cpu_interface_gicv2::init(registers.as_mut());

                Ok(Self::V2 {
                    registers,
                    cpu_index: cpu_index as u16,
                })
            },
            Version::InitV3 { redist, .. } => {
                let phys_addr = redist[cpu_index];

                let mut redistributor: BorrowedMappedPages<GicRegisters, Mutable> = {
                    let mapped = map_frame_range(phys_addr, PAGE_SIZE, MMIO_FLAGS)?;
                    mapped.into_borrowed_mut(0).map_err(|(_, e)| e)?
                };

                let redist_sgippi = {
                    let rso_paddr = phys_addr + REDIST_SGIPPI_OFFSET;
                    let mapped = map_frame_range(rso_paddr, PAGE_SIZE, MMIO_FLAGS)?;
                    mapped.into_borrowed_mut(0).map_err(|(_, e)| e)?
                };

                redist_interface::init(redistributor.as_mut())?;
                cpu_interface_gicv3::init();

                Ok(Self::V3 {
                    redist_regs: ArmGicV3RedistPages {
                        redistributor,
                        redist_sgippi,
                    },
                })
            },
        }
    }

    pub fn init_secondary_cpu_interface(&mut self) {
        match self {
            Self::V2 { registers, .. } => cpu_interface_gicv2::init(registers.as_mut()),
            Self::V3 { .. } => cpu_interface_gicv3::init(),
        }
    }

    /// Sends an inter processor interrupt (IPI),
    /// also called software generated interrupt (SGI).
    ///
    /// note: on Aarch64, IPIs must have a number below 16 on ARMv8
    pub fn send_ipi(&mut self, int_num: InterruptNumber, target: IpiTargetCpu) {
        assert!(int_num < 16, "IPIs must have a number below 16 on ARMv8");

        if let Self::V3 { .. } = self {
            cpu_interface_gicv3::send_ipi(int_num, target)
        } else {
            // we don't have access to the distributor... code would be:
            // dist_interface::send_ipi_gicv2(&mut dist_regs, int_num, target)
            // workaround: caller could check is this must be done in the dist
            // and then get the SystemInterruptController and call a dedicated
            // method on it, like `sys_ctlr.send_ipi_gicv2()`

            panic!("GICv2 doesn't support sending IPIs (need distributor)");
        }
    }

    /// Acknowledge the currently serviced interrupt
    /// and fetches its number
    pub fn acknowledge_interrupt(&mut self) -> (InterruptNumber, Priority) {
        match self {
            Self::V2 { registers, .. } => cpu_interface_gicv2::acknowledge_interrupt(registers),
            Self::V3 { .. } => cpu_interface_gicv3::acknowledge_interrupt(),
        }
    }

    /// Performs priority drop for the specified interrupt
    pub fn end_of_interrupt(&mut self, int: InterruptNumber) {
        match self {
            Self::V2 { registers, .. } => cpu_interface_gicv2::end_of_interrupt(registers, int),
            Self::V3 { .. } => cpu_interface_gicv3::end_of_interrupt(int),
        }
    }

    /// Will that interrupt be received by this CPU?
    pub fn get_interrupt_state(&self, int: InterruptNumber) -> Enabled {
        assert!(int < 32);

        if let Self::V3 { redist_regs } = self {
            redist_interface::is_sgippi_enabled(&redist_regs.redist_sgippi, int)
        } else {
            // there is no redistributor and we don't have access to the distributor
            log::error!("GICv2 doesn't support enabling/disabling local interrupt");

            // should we panic?
            true
        }
    }

    /// Enables or disables the receiving of a local interrupt in the distributor
    pub fn set_interrupt_state(&mut self, int: InterruptNumber, enabled: Enabled) {
        assert!(int < 32);

        if let Self::V3 { redist_regs } = self {
            redist_interface::enable_sgippi(&mut redist_regs.redist_sgippi, int, enabled);
        } else {
            // there is no redistributor and we don't have access to the distributor
            log::error!("GICv2 doesn't support enabling/disabling local interrupt");
        }
    }

    /// Returns the priority of a local interrupt
    pub fn get_interrupt_priority(&self, int: InterruptNumber) -> Priority {
        assert!(int < 32);

        if let Self::V3 { redist_regs } = self {
            redist_interface::get_sgippi_priority(&redist_regs.redist_sgippi, int)
        } else {
            // there is no redistributor and we don't have access to the distributor
            log::error!("GICv2 doesn't support setting local interrupt priority");

            // should we panic?
            128
        }
    }

    /// Sets the priority of a local interrupt (prio: 0-255)
    pub fn set_interrupt_priority(&mut self, int: InterruptNumber, enabled: Priority) {
        assert!(int < 32);

        if let Self::V3 { redist_regs } = self {
            redist_interface::set_sgippi_priority(&mut redist_regs.redist_sgippi, int, enabled);
        } else {
            // there is no redistributor and we don't have access to the distributor
            log::error!("GICv2 doesn't support setting local interrupt priority");
        }
    }

    /// Interrupts have a priority; if their priority
    /// is lower or equal to this one, they're queued
    /// until this CPU or another one is ready to handle
    /// them
    pub fn get_minimum_priority(&self) -> Priority {
        match self {
            Self::V2 { registers, .. } => cpu_interface_gicv2::get_minimum_priority(&registers),
            Self::V3 { .. } => cpu_interface_gicv3::get_minimum_priority(),
        }
    }

    /// Interrupts have a priority; if their priority
    /// is lower or equal to this one, they're queued
    /// until this CPU or another one is ready to handle
    /// them
    pub fn set_minimum_priority(&mut self, priority: Priority) {
        match self {
            Self::V2 { registers, .. } => cpu_interface_gicv2::set_minimum_priority(registers, priority),
            Self::V3 { .. } => cpu_interface_gicv3::set_minimum_priority(priority),
        }
    }

    /// Returns the internal ID of the redistributor (GICv3)
    ///
    /// Note #1: as a compatibility feature, on GICv2, the CPU index is returned.
    /// Note #2: this is only provided for debugging purposes
    pub fn get_cpu_interface_id(&self) -> u16 {
        match self {
            Self::V3 { redist_regs } => redist_interface::get_internal_id(&redist_regs.redistributor),
            Self::V2 { cpu_index, .. } => *cpu_index,
        }
    }
}

impl core::fmt::Debug for ArmGicV3RedistPages {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "ArmGicV3RedistPages")
    }
}
