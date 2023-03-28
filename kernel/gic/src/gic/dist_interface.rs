//! Distributor Interface
//!
//! The Distributor forwards or discards SPIs (shared peripheral interrupts)
//! to:
//! - The redistributor, in GICv3,
//! - The CPU core, in GICv2.
//! There's one Distributor per GIC chip.
//!
//! Included functionality:
//! - Initializing the interface
//! - Enabling or disabling the forwarding of SPIs based on their numbers
//! - Setting the target of SPIs based on their numbers
//! - Generating software interrupts (GICv2 style)

use super::GicRegisters;
use super::{U32BYTES, U64BYTES};
use super::SpiDestination;
use super::IpiTargetCpu;
use super::InterruptNumber;
use super::Enabled;
use super::TargetList;

use cpu::MpidrValue;

mod offset {
    use super::{U32BYTES, U64BYTES};
    pub const CTLR:      usize = 0x000 / U32BYTES;
    pub const IGROUPR:   usize = 0x080 / U32BYTES;
    pub const ISENABLER: usize = 0x100 / U32BYTES;
    pub const ICENABLER: usize = 0x180 / U32BYTES;
    pub const ITARGETSR: usize = 0x800 / U32BYTES;
    pub const SGIR:      usize = 0xf00 / U32BYTES;
    /// This one is on the 6th page
    pub const P6IROUTER: usize = 0x100 / U64BYTES;
}

// enable group 0
// const CTLR_ENGRP0: u32 = 0b01;

// enable group 1
const CTLR_ENGRP1: u32 = 0b10;

// Affinity Routing Enable, Non-secure state.
const CTLR_ARE_NS: u32 = 1 << 5;

// bit 24: target mode
//   1 = all other PEs
//   0 = use target list
const SGIR_TARGET_ALL_OTHER_PE: u32 = 1 << 24;

// bit 31: SPI routing
//   1 = any available PE
//   0 = route to specific PE
const P6IROUTER_ANY_AVAILABLE_PE: u64 = 1 << 31;

// const GROUP_0: u32 = 0;
const GROUP_1: u32 = 1;

// bit 15: which interrupt group to target
const SGIR_NSATT_GRP1: u32 = 1 << 15;

/// Initializes the distributor by enabling forwarding
/// of group 1 interrupts
///
/// Return value: whether or not affinity routing is
/// currently enabled for both secure and non-secure
/// states.
pub fn init(registers: &mut GicRegisters) -> Enabled {
    let mut reg = registers.read_volatile(offset::CTLR);
    reg |= CTLR_ENGRP1;
    registers.write_volatile(offset::CTLR, reg);

    // Return value: whether or not affinity routing is
    // currently enabled for both secure and non-secure
    // states.
    reg & CTLR_ARE_NS > 0
}

/// Returns whether the given SPI (shared peripheral interrupt) will be
/// forwarded by the distributor
pub fn is_spi_enabled(registers: &GicRegisters, int: InterruptNumber) -> Enabled {
    // enabled?
    registers.read_array_volatile::<32>(offset::ISENABLER, int) > 0
    &&
    // part of group 1?
    registers.read_array_volatile::<32>(offset::IGROUPR, int) == GROUP_1
}

/// Enables or disables the forwarding of a particular SPI (shared peripheral interrupt)
pub fn enable_spi(registers: &mut GicRegisters, int: InterruptNumber, enabled: Enabled) {
    let reg_base = match enabled {
        true  => offset::ISENABLER,
        false => offset::ICENABLER,
    };
    registers.write_array_volatile::<32>(reg_base, int, 1);

    // whether we're enabling or disabling,
    // set as part of group 1
    registers.write_array_volatile::<32>(reg_base, int, GROUP_1);
}

/// Sends an Inter-Processor-Interrupt
///
/// legacy / GICv2 method
/// int_num must be less than 16
pub fn send_ipi_gicv2(registers: &mut GicRegisters, int_num: u32, target: IpiTargetCpu) {
    if let IpiTargetCpu::Specific(cpu) = target {
        assert!(cpu.value() < 8, "affinity routing is disabled; cannot target a CPU with id >= 8");
    }

    let target_list = match target {
        IpiTargetCpu::Specific(cpu) => (1 << cpu.value()) << 16,
        IpiTargetCpu::AllOtherCpus => SGIR_TARGET_ALL_OTHER_PE,
        IpiTargetCpu::GICv2TargetList(list) => (list.bits as u32) << 16,
    };

    let value: u32 = int_num | target_list | SGIR_NSATT_GRP1;
    registers.write_volatile(offset::SGIR, value);
}

impl super::ArmGic {
    pub(crate) fn distributor(&self) -> &GicRegisters {
        match self {
            Self::V2(v2) => &v2.distributor,
            Self::V3(v3) => &v3.distributor,
        }
    }

    pub(crate) fn distributor_mut(&mut self) -> &mut GicRegisters {
        match self {
            Self::V2(v2) => &mut v2.distributor,
            Self::V3(v3) => &mut v3.distributor,
        }
    }

    /// The GIC can be configured to route
    /// Shared-Peripheral Interrupts (SPI) either
    /// to a specific CPU or to any PE that is ready
    /// to process them, i.e. not handling another
    /// higher-priority interrupt.
    pub fn get_spi_target(&self, int: InterruptNumber) -> SpiDestination {
        assert!(int >= 32, "interrupts number below 32 (SGIs & PPIs) don't have a target CPU");
        if !self.affinity_routing() {
            let flags = self.distributor().read_array_volatile::<4>(offset::ITARGETSR, int);
            if flags == 0xff {
                return SpiDestination::AnyCpuAvailable;
            }

            for i in 0..8 {
                let target = 1 << i;
                if target & flags == target {
                    return SpiDestination::Specific(MpidrValue::new(0, 0, 0, i).into());
                }
            }

            let list = TargetList::from_bits_truncate(flags as u8);
            SpiDestination::GICv2TargetList(list)
        } else if let Self::V3(v3) = self {
            let reg = v3.dist_extended.read_volatile_64(offset::P6IROUTER);

            // bit 31: Interrupt Routing Mode
            // value of 1 to target any available cpu
            // value of 0 to target a specific cpu
            if reg & P6IROUTER_ANY_AVAILABLE_PE > 0 {
                SpiDestination::AnyCpuAvailable
            } else {
                let aff3 = (reg >> 32) as u8;
                let aff2 = (reg >> 16) as u8;
                let aff1 = (reg >>  8) as u8;
                let aff0 = (reg >>  0) as u8;
                SpiDestination::Specific(MpidrValue::new(aff3, aff2, aff1, aff0).into())
            }
        } else {
            // If we're on gicv2 then affinity routing is off
            // so we landed in the first block
            unreachable!()
        }
    }

    /// The GIC can be configured to route
    /// Shared-Peripheral Interrupts (SPI) either
    /// to a specific CPU or to any PE that is ready
    /// to process them, i.e. not handling another
    /// higher-priority interrupt.
    pub fn set_spi_target(&mut self, int: InterruptNumber, target: SpiDestination) {
        assert!(int >= 32, "interrupts number below 32 (SGIs & PPIs) don't have a target CPU");
        if !self.affinity_routing() {
            if let SpiDestination::Specific(cpu) = target {
                assert!(cpu.value() < 8, "affinity routing is disabled; cannot target a CPU with id >= 8");
            }

            let value = match target {
                SpiDestination::Specific(cpu) => 1 << cpu.value(),
                SpiDestination::AnyCpuAvailable => 0xff,
                SpiDestination::GICv2TargetList(list) => list.bits as u32,
            };

            self.distributor_mut().write_array_volatile::<4>(offset::ITARGETSR, int, value);
        } else if let Self::V3(v3) = self {
            let value = match target {
                SpiDestination::Specific(cpu) => MpidrValue::from(cpu).value(),
                // bit 31: Interrupt Routing Mode
                // value of 1 to target any available cpu
                SpiDestination::AnyCpuAvailable => P6IROUTER_ANY_AVAILABLE_PE,
                SpiDestination::GICv2TargetList(_) => {
                    panic!("Cannot use SpiDestination::GICv2TargetList with GICv3!");
                },
            };

            v3.dist_extended.write_volatile_64(offset::P6IROUTER, value);
        }

        // If we're on gicv2 then affinity routing is off
        // so we landed in the first block
        unreachable!()
    }
}
