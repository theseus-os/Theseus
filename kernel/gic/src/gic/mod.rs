use arm_boards::{BOARD_CONFIG, NUM_CPUS};
use cpu::{CpuId, MpidrValue};
use volatile::Volatile;
use spin::Once;

use memory::{
    BorrowedMappedPages, Mutable, PhysicalAddress, MappedPages,
    AllocatedFrames, get_kernel_mmi_ref, allocate_frames_at,
    allocate_pages, map_frame_range, PAGE_SIZE, MMIO_FLAGS,
};


mod cpu_interface_gicv3;
mod cpu_interface_gicv2;
mod dist_interface;
mod redist_interface;

use dist_interface::{DistRegsP1, DistRegsP6};
use cpu_interface_gicv2::CpuRegsP1;
use redist_interface::{RedistRegsP1, RedistRegsSgiPpi};

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

// Reads one item of an array spanning across
// multiple u32s.
//
// The maximum item size is 32 bits, and the items are always aligned to 2**N bits.
// The array spans multiple adjacent u32s but there is always a integer number of
// items in a single u32.
//
// - `int` is the index
// - `INTS_PER_U32` = how many array slots per u32 in this array
fn read_array_volatile<const INTS_PER_U32: usize>(slice: &[Volatile<u32>], int: InterruptNumber) -> u32 {
    let int = int as usize;
    let bits_per_int: usize = U32BITS / INTS_PER_U32;
    let mask: u32 = u32::MAX >> (U32BITS - bits_per_int);

    let offset = int / INTS_PER_U32;
    let reg_index = int & (INTS_PER_U32 - 1);
    let shift = reg_index * bits_per_int;

    let reg = slice[offset].read();
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
// - `INTS_PER_U32` = how many array slots per u32 in this array
// - `value` is the value to write
fn write_array_volatile<const INTS_PER_U32: usize>(slice: &mut [Volatile<u32>], int: InterruptNumber, value: u32) {
    let int = int as usize;
    let bits_per_int: usize = U32BITS / INTS_PER_U32;
    let mask: u32 = u32::MAX >> (U32BITS - bits_per_int);

    let offset = int / INTS_PER_U32;
    let reg_index = int & (INTS_PER_U32 - 1);
    let shift = reg_index * bits_per_int;

    let mut reg = slice[offset].read();
    reg &= !(mask << shift);
    reg |= (value & mask) << shift;
    slice[offset].write(reg);
}

const REDIST_SGIPPI_OFFSET: usize = 0x10000;
const DIST_P6_OFFSET: usize = 0x6000;

pub struct ArmGicV3RedistPages {
    pub redistributor: BorrowedMappedPages<RedistRegsP1, Mutable>,
    pub redist_sgippi: BorrowedMappedPages<RedistRegsSgiPpi, Mutable>,
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
        registers: BorrowedMappedPages<DistRegsP1, Mutable>,
    },
    V3 {
        affinity_routing: Enabled,
        v2_regs: BorrowedMappedPages<DistRegsP1, Mutable>,
        v3_regs: BorrowedMappedPages<DistRegsP6, Mutable>,
    },
}

impl ArmGicDistributor {
    pub fn init(version: &Version) -> Result<Self, &'static str> {
        match version {
            Version::InitV2 { dist, .. } => {
                let mapped = map_frame_range(*dist, PAGE_SIZE, MMIO_FLAGS)?;
                let mut registers = mapped
                    .into_borrowed_mut::<DistRegsP1>(0)
                    .map_err(|(_, e)| e)?;

                registers.init();

                Ok(Self::V2 {
                    registers,
                })
            },
            Version::InitV3 { dist, .. } => {
                let mapped = map_frame_range(*dist, PAGE_SIZE, MMIO_FLAGS)?;
                let mut v2_regs = mapped
                    .into_borrowed_mut::<DistRegsP1>(0)
                    .map_err(|(_, e)| e)?;

                let v3_regs: BorrowedMappedPages<DistRegsP6, Mutable> = {
                    let paddr = *dist + DIST_P6_OFFSET;
                    let mapped = map_frame_range(paddr, PAGE_SIZE, MMIO_FLAGS)?;
                    mapped.into_borrowed_mut(0).map_err(|(_, e)| e)?
                };

                let affinity_routing = v2_regs.init();

                Ok(Self::V3 {
                    affinity_routing,
                    v2_regs,
                    v3_regs,
                })
            },
        }
    }

    /// Returns whether the given interrupt is forwarded by the distributor.
    ///
    /// Panics if `int` is not in the SPI range (>= 32).
    pub fn get_spi_state(&self, int: InterruptNumber) -> Enabled {
        assert!(int >= 32, "get_spi_state: `int` must be >= 32");
        self.distributor().is_spi_enabled(int)
    }

    /// Enables or disables the forwarding of the given interrupt
    /// by the distributor.
    ///
    /// Panics if `int` is not in the SPI range (>= 32).
    pub fn set_spi_state(&mut self, int: InterruptNumber, enabled: Enabled) {
        assert!(int >= 32, "set_spi_state: `int` must be >= 32");
        self.distributor_mut().enable_spi(int, enabled)
    }

    /// Returns the priority of the given interrupt.
    ///
    /// Panics if `int` is not in the SPI range (>= 32).
    pub fn get_spi_priority(&self, int: InterruptNumber) -> Priority {
        assert!(int >= 32, "get_spi_priority: `int` must be >= 32");
        self.distributor().get_spi_priority(int)
    }

    /// Sets the priority of the given interrupt.
    ///
    /// Panics if `int` is not in the SPI range (>= 32).
    pub fn set_spi_priority(&mut self, int: InterruptNumber, enabled: Priority) {
        assert!(int >= 32, "set_spi_priority: `int` must be >= 32");
        self.distributor_mut().set_spi_priority(int, enabled)
    }
}

pub enum ArmGicCpuComponents {
    V2 {
        registers: BorrowedMappedPages<CpuRegsP1, Mutable>,
        cpu_index: u16,
    },
    V3 {
        redist_regs: ArmGicV3RedistPages,
    },
}

/// Map the physical frames containing the GICv2 CPU Interface's MMIO registers into the given `page_table`.
fn map_gicv2_cpu_iface(cpu_iface: PhysicalAddress) -> Result<MappedPages, &'static str> {
    static CPU_IFACE_FRAME: Once<AllocatedFrames> = Once::new();

    let frame = if let Some(cpu_iface) = CPU_IFACE_FRAME.get() {
        cpu_iface
    } else {
        let cpu_iface = allocate_frames_at(cpu_iface, 1)?;
        CPU_IFACE_FRAME.call_once(|| cpu_iface)
    };

    let new_page = allocate_pages(1).ok_or("out of virtual address space!")?;
    let mmi = get_kernel_mmi_ref().ok_or("map_gicv2_cpu_iface(): uninitialized KERNEL_MMI")?;
    let mut mmi = mmi.lock();

    // The CPU Interface frame is the same actual physical address across all CPU cores,
    // but they're actually completely independent pieces of hardware that share one address.
    // Therefore, there's no way to represent that to the Rust language or
    // MappedPages/AllocatedFrames types, so we must use unsafe code, at least for now.
    unsafe {
        memory::Mapper::map_to_non_exclusive(
            &mut mmi.page_table,
            new_page,
            frame,
            MMIO_FLAGS,
        )
    }
}

impl ArmGicCpuComponents {
    pub fn init(cpu_id: CpuId, version: &Version) -> Result<Self, &'static str> {
        let cpu_index = BOARD_CONFIG.cpu_ids.iter()
            .position(|mpidr| CpuId::from(*mpidr) == cpu_id)
            .expect("BUG: invalid CpuId in ArmGicCpuComponents::init");

        match version {
            Version::InitV2 { cpu, .. } => {
                let mut registers: BorrowedMappedPages<CpuRegsP1, Mutable> = {
                    let mapped = map_gicv2_cpu_iface(*cpu)?;
                    mapped.into_borrowed_mut(0).map_err(|(_, e)| e)?
                };

                registers.init();

                Ok(Self::V2 {
                    registers,
                    cpu_index: cpu_index as u16,
                })
            },
            Version::InitV3 { redist, .. } => {
                let phys_addr = redist[cpu_index];

                let mut redistributor: BorrowedMappedPages<RedistRegsP1, Mutable> = {
                    let mapped = map_frame_range(phys_addr, PAGE_SIZE, MMIO_FLAGS)?;
                    mapped.into_borrowed_mut(0).map_err(|(_, e)| e)?
                };

                let redist_sgippi = {
                    let rso_paddr = phys_addr + REDIST_SGIPPI_OFFSET;
                    let mapped = map_frame_range(rso_paddr, PAGE_SIZE, MMIO_FLAGS)?;
                    mapped.into_borrowed_mut(0).map_err(|(_, e)| e)?
                };

                redistributor.init()?;
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
            Self::V2 { registers, .. } => registers.init(),
            Self::V3 { .. } => cpu_interface_gicv3::init(),
        }
    }

    /// Sends an Inter-Processor Interrupt (IPI) with the given interrupt number
    /// to the given target CPU(s).
    ///
    /// This is also referred to as a Software-Generated Interrupt (SGI).
    ///
    /// Panics if `int` is greater than or equal to 16;
    /// on aarch64, IPIs much be sent to an interrupt number less than 16.
    pub fn send_ipi(&mut self, int: InterruptNumber, target: IpiTargetCpu) {
        assert!(int < 16, "IPIs must have a number below 16 on ARMv8");

        if let Self::V3 { .. } = self {
            cpu_interface_gicv3::send_ipi(int, target)
        } else {
            // we don't have access to the distributor... code would be:
            // dist_interface::send_ipi_gicv2(&mut dist_regs, int, target)
            // workaround: caller could check is this must be done in the dist
            // and then get the SystemInterruptController and call a dedicated
            // method on it, like `sys_ctlr.send_ipi_gicv2()`

            panic!("GICv2 doesn't support sending IPIs (need distributor)");
        }
    }

    /// Acknowledge the currently-serviced interrupt.
    ///
    /// This tells the GIC that the current interrupt is in the midst of
    /// being handled by this CPU.
    ///
    /// Returns a tuple of the interrupt's number and priority.
    pub fn acknowledge_interrupt(&mut self) -> (InterruptNumber, Priority) {
        match self {
            Self::V2 { registers, .. } => registers.acknowledge_interrupt(),
            Self::V3 { .. } => cpu_interface_gicv3::acknowledge_interrupt(),
        }
    }

    /// Signals to the controller that the currently processed interrupt
    /// has been fully handled, by zeroing the current priority level of
    /// the current CPU.
    ///
    /// This implies that the CPU is ready to process interrupts again.
    pub fn end_of_interrupt(&mut self, int: InterruptNumber) {
        match self {
            Self::V2 { registers, .. } => registers.end_of_interrupt(int),
            Self::V3 { .. } => cpu_interface_gicv3::end_of_interrupt(int),
        }
    }

    /// Returns whether the given local interrupt will be received by the current CPU.
    ///
    /// Panics if `int` is greater than or equal to 32, which is beyond the range
    /// of local interrupt numbers.
    pub fn get_interrupt_state(&self, int: InterruptNumber) -> Enabled {
        assert!(int < 32, "get_interrupt_state: `int` doesn't lie in the SGI/PPI (local interrupt) range");

        if let Self::V3 { redist_regs } = self {
            redist_regs.redist_sgippi.is_sgippi_enabled(int)
        } else {
            // there is no redistributor and we don't have access to the distributor
            log::error!("GICv2 doesn't support enabling/disabling local interrupt");

            // should we panic?
            true
        }
    }

    /// Enables or disables the receiving of a local interrupt in the distributor.
    ///
    /// Panics if `int` is greater than or equal to 32, which is beyond the range
    /// of local interrupt numbers.
    pub fn set_interrupt_state(&mut self, int: InterruptNumber, enabled: Enabled) {
        assert!(int < 32, "set_interrupt_state: `int` doesn't lie in the SGI/PPI (local interrupt) range");

        if let Self::V3 { redist_regs } = self {
            redist_regs.redist_sgippi.enable_sgippi(int, enabled);
        } else {
            // there is no redistributor and we don't have access to the distributor
            log::error!("GICv2 doesn't support enabling/disabling local interrupt");
        }
    }

    /// Returns the priority of a local interrupt
    ///
    /// Panics if `int` is greater than or equal to 32, which is beyond the range
    /// of local interrupt numbers.
    pub fn get_interrupt_priority(&self, int: InterruptNumber) -> Priority {
        assert!(int < 32, "get_interrupt_priority: `int` doesn't lie in the SGI/PPI (local interrupt) range");

        if let Self::V3 { redist_regs } = self {
            redist_regs.redist_sgippi.get_sgippi_priority(int)
        } else {
            // there is no redistributor and we don't have access to the distributor
            log::error!("GICv2 doesn't support setting local interrupt priority");

            // should we panic?
            128
        }
    }

    /// Sets the priority of a local interrupt (prio: 0-255)
    ///
    /// Panics if `int` is greater than or equal to 32, which is beyond the range
    /// of local interrupt numbers.
    pub fn set_interrupt_priority(&mut self, int: InterruptNumber, enabled: Priority) {
        assert!(int < 32, "set_interrupt_priority: `int` doesn't lie in the SGI/PPI (local interrupt) range");

        if let Self::V3 { redist_regs } = self {
            redist_regs.redist_sgippi.set_sgippi_priority(int, enabled);
        } else {
            // there is no redistributor and we don't have access to the distributor
            log::error!("GICv2 doesn't support setting local interrupt priority");
        }
    }

    /// Retrieves the current priority threshold for the current CPU.
    ///
    /// Interrupts have a priority; if their priority is lower or
    /// equal to this threshold, they're queued until the current CPU
    /// is ready to handle them.
    pub fn get_minimum_priority(&self) -> Priority {
        match self {
            Self::V2 { registers, .. } => registers.get_minimum_priority(),
            Self::V3 { .. } => cpu_interface_gicv3::get_minimum_priority(),
        }
    }

    /// Sets the current priority threshold for the current CPU.
    ///
    /// Interrupts have a priority; if their priority is lower or
    /// equal to this threshold, they're queued until the current CPU
    /// is ready to handle them.
    pub fn set_minimum_priority(&mut self, priority: Priority) {
        match self {
            Self::V2 { registers, .. } => registers.set_minimum_priority(priority),
            Self::V3 { .. } => cpu_interface_gicv3::set_minimum_priority(priority),
        }
    }

    /// Returns the internal ID of the redistributor (GICv3)
    ///
    /// ## Notes
    /// * As a compatibility feature, on GICv2, the CPU index is returned.
    /// * This is only provided for debugging purposes.
    pub fn get_cpu_interface_id(&self) -> u16 {
        match self {
            Self::V3 { redist_regs } => redist_regs.redistributor.get_internal_id(),
            Self::V2 { cpu_index, .. } => *cpu_index,
        }
    }
}

impl core::fmt::Debug for ArmGicV3RedistPages {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "ArmGicV3RedistPages")
    }
}
