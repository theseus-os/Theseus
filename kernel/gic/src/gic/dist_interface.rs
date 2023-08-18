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
//! - Getting or setting the priority of SPIs based on their numbers
//! - Getting or setting the target of SPIs based on their numbers
//! - Generating software interrupts (GICv2 style)

use super::IpiTargetCpu;
use super::SpiDestination;
use super::InterruptNumber;
use super::Enabled;
use super::Priority;
use super::TargetList;
use super::read_array_volatile;
use super::write_array_volatile;

use volatile::{Volatile, ReadOnly};
use zerocopy::FromBytes;
use cpu::MpidrValue;

/// First page of distributor registers
#[derive(FromBytes)]
#[repr(C)]
pub struct DistRegsP1 {                   // base offset
    /// Distributor Control Register
    ctlr:          Volatile<u32>,         // 0x000

    /// Interrupt Controller Type Register
    typer:         ReadOnly<u32>,         // 0x004

    /// Distributor Implementer Identification Register
    ident:         ReadOnly<u32>,         // 0x008

    _unused0:     [u8;            0x074],

    /// Interrupt Group Registers
    group:        [Volatile<u32>; 0x020], // 0x080

    /// Interrupt Set-Enable Registers
    set_enable:   [Volatile<u32>; 0x020], // 0x100

    /// Interrupt Clear-Enable Registers
    clear_enable: [Volatile<u32>; 0x020], // 0x180

    _unused1:     [u8;            0x200],

    /// Interrupt Priority Registers
    priority:     [Volatile<u32>; 0x100], // 0x400

    /// Interrupt Processor Targets Registers
    target:       [Volatile<u32>; 0x100], // 0x800

    _unused2:     [u8;            0x300],

    /// Software Generated Interrupt Register
    sgir:          Volatile<u32>,         // 0xf00
}

/// Sixth page of distributor registers
#[derive(FromBytes)]
#[repr(C)]
pub struct DistRegsP6 {     // base offset
    _unused: [u8; 0x100],

    /// Interrupt Routing Registers
    route:   Volatile<u64>, // 0x100
}

// enable group 0
// const CTLR_ENGRP0: u32 = 0b01;

// enable group 1
const CTLR_ENGRP1: u32 = 0b10;

// enable 1 of N wakeup functionality
const CTLR_E1NWF: u32 = 1 << 7;

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

impl DistRegsP1 {
    /// Initializes the distributor by enabling forwarding
    /// of group 1 interrupts and allowing the GIC to pick
    /// a core that is asleep for "1 of N" interrupts.
    ///
    /// Return value: whether or not affinity routing is
    /// currently enabled for both secure and non-secure
    /// states.
    pub fn init(&mut self) -> Enabled {
        let mut reg = self.ctlr.read();
        reg |= CTLR_ENGRP1;
        reg |= CTLR_E1NWF;
        self.ctlr.write(reg);

        // Return value: whether or not affinity routing is
        // currently enabled for both secure and non-secure
        // states.
        reg & CTLR_ARE_NS > 0
    }

    /// Returns whether the given SPI (shared peripheral interrupt) will be
    /// forwarded by the distributor
    pub fn is_spi_enabled(&self, int: InterruptNumber) -> Enabled {
        // enabled?
        read_array_volatile::<32>(&self.set_enable, int) > 0
        &&
        // part of group 1?
        read_array_volatile::<32>(&self.group, int) == GROUP_1
    }

    /// Enables or disables the forwarding of a particular SPI (shared peripheral interrupt)
    pub fn enable_spi(&mut self, int: InterruptNumber, enabled: Enabled) {
        let reg_base = match enabled {
            true  => &mut self.set_enable,
            false => &mut self.clear_enable,
        };
        write_array_volatile::<32>(reg_base, int, 1);

        // whether we're enabling or disabling,
        // set as part of group 1
        write_array_volatile::<32>(&mut self.group, int, GROUP_1);
    }

    /// Returns the priority of an SPI.
    pub fn get_spi_priority(&self, int: InterruptNumber) -> Priority {
        u8::MAX - (read_array_volatile::<4>(&self.priority, int) as u8)
    }

    /// Sets the priority of an SPI.
    pub fn set_spi_priority(&mut self, int: InterruptNumber, prio: Priority) {
        write_array_volatile::<4>(&mut self.priority, int, (u8::MAX - prio) as u32);
    }

    /// Sends an Inter-Processor-Interrupt
    ///
    /// legacy / GICv2 method
    /// int_num must be less than 16
    #[allow(dead_code)]
    pub fn send_ipi_gicv2(&mut self, int_num: u32, target: IpiTargetCpu) {
        if let IpiTargetCpu::Specific(cpu) = &target {
            assert!(cpu.value() < 8, "affinity routing is disabled; cannot target a CPU with id >= 8");
        }

        let target_list = match target {
            IpiTargetCpu::Specific(cpu) => (1 << cpu.value()) << 16,
            IpiTargetCpu::AllOtherCpus => SGIR_TARGET_ALL_OTHER_PE,
            IpiTargetCpu::GICv2TargetList(list) => (list.0 as u32) << 16,
        };

        let value: u32 = int_num | target_list | SGIR_NSATT_GRP1;
        self.sgir.write(value);
    }
}

/// Deserialized content of the `IIDR` distributor register
pub struct Implementer {
    /// Product Identifier of this distributor.
    pub product_id: u8,
    /// An arbitrary revision number defined by the implementer.
    pub version: u8,
    /// Contains the JEP106 code of the company that implemented the distributor.
    pub implementer_jep106: u16,
}

impl super::ArmGicDistributor {
    pub(crate) fn distributor(&self) -> &DistRegsP1 {
        match self {
            Self::V2 { registers } => registers,
            Self::V3 { v2_regs, .. } => v2_regs,
        }
    }

    pub(crate) fn distributor_mut(&mut self) -> &mut DistRegsP1 {
        match self {
            Self::V2 { registers } => registers,
            Self::V3 { v2_regs, .. } => v2_regs,
        }
    }

    pub fn implementer(&self) -> Implementer {
        let raw = self.distributor().ident.read();
        Implementer {
            product_id: (raw >> 24) as _,
            version: ((raw >> 12) & 0xff) as _,
            implementer_jep106: (raw & 0xfff) as _,
        }
    }

    /// Returns the destination of an SPI if it's valid, i.e. if it
    /// points to existing CPU(s).
    ///
    /// Note: If the destination is a `GICv2TargetList`, the validity
    /// of that destination is not checked.
    pub fn get_spi_target(&self, int: InterruptNumber) -> Result<SpiDestination, &'static str> {
        assert!(int >= 32, "interrupts number below 32 (SGIs & PPIs) don't have a target CPU");
        match self {
            Self::V2 { .. } | Self::V3 { affinity_routing: false, .. } => {
                let flags = read_array_volatile::<4>(&self.distributor().target, int);

                for i in 0..8 {
                    let target = 1 << i;
                    if target & flags == flags {
                        let mpidr = i;
                        let cpu_id = MpidrValue::try_from(mpidr)?.into();
                        return Ok(SpiDestination::Specific(cpu_id));
                    }
                }

                Ok(SpiDestination::GICv2TargetList(TargetList(flags as u8)).canonicalize())
            },
            Self::V3 { affinity_routing: true, v3_regs, .. } => {
                let reg = v3_regs.route.read();

                // bit 31: Interrupt Routing Mode
                // value of 1 to target any available cpu
                // value of 0 to target a specific cpu
                if reg & P6IROUTER_ANY_AVAILABLE_PE > 0 {
                    Ok(SpiDestination::AnyCpuAvailable)
                } else {
                    let mpidr = reg & 0xff00ffffff;
                    let cpu_id = MpidrValue::try_from(mpidr)?.into();
                    Ok(SpiDestination::Specific(cpu_id))
                }
            }
        }
    }

    /// Sets the destination of an SPI.
    pub fn set_spi_target(&mut self, int: InterruptNumber, target: SpiDestination) {
        assert!(int >= 32, "interrupts number below 32 (SGIs & PPIs) don't have a target CPU");
        match self {
            Self::V2 { .. } | Self::V3 { affinity_routing: false, .. } => {
                if let SpiDestination::Specific(cpu) = &target {
                    assert!(cpu.value() < 8, "affinity routing is disabled; cannot target a CPU with id >= 8");
                }

                let value = match target {
                    SpiDestination::Specific(cpu) => 1 << cpu.value(),
                    SpiDestination::AnyCpuAvailable => {
                        let list = TargetList::new_all_cpus()
                            .expect("This is invalid: CpuId > 8 AND affinity routing is disabled");
                        list.0 as u32
                    },
                    SpiDestination::GICv2TargetList(list) => list.0 as u32,
                };

                write_array_volatile::<4>(&mut self.distributor_mut().target, int, value);
            },
            Self::V3 { affinity_routing: true, v3_regs, .. } => {
                let value = match target {
                    SpiDestination::Specific(cpu) => MpidrValue::from(cpu).value(),
                    // bit 31: Interrupt Routing Mode
                    // value of 1 to target any available cpu
                    SpiDestination::AnyCpuAvailable => P6IROUTER_ANY_AVAILABLE_PE,
                    SpiDestination::GICv2TargetList(_) => {
                        panic!("Cannot use SpiDestination::GICv2TargetList with affinity routing enabled");
                    },
                };

                v3_regs.route.write(value);
            }
        }
    }
}
