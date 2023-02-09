use super::MmioPageOfU32;
use super::U32BYTES;
use super::TargetCpu;
use super::IntNumber;
use super::Enabled;
use super::read_array;
use super::write_array;
use super::TargetList;

mod offset {
    use super::U32BYTES;
    pub const CTLR:      usize = 0x000 / U32BYTES;
    pub const IGROUPR:   usize = 0x080 / U32BYTES;
    pub const ISENABLER: usize = 0x100 / U32BYTES;
    pub const ICENABLER: usize = 0x180 / U32BYTES;
    pub const ITARGETSR: usize = 0x800 / U32BYTES;
    pub const SGIR:      usize = 0xf00 / U32BYTES;
    /// This one is on the 6th page
    pub const P6IROUTER: usize = 0x100 / U32BYTES;
}

// enable group 0
// const CTLR_ENGRP0: u32 = 0b01;

// enable group 1
const CTLR_ENGRP1: u32 = 0b10;

// Affinity Routing Enable, Non-secure state.
const CTLR_ARE_NS: u32 = 1 << 5;

// bits [24:25]: target mode
//   1 = all other PEs
//   0 = use target list
const SGIR_TARGET_ALL_OTHER_PE: u32 = 1 << 24;

// const GROUP_0: u32 = 0;
const GROUP_1: u32 = 1;

// bit 15: which interrupt group to target
const SGIR_NSATT_GRP0: u32 = 0 << 15;

fn assert_cpu_bounds(target: &TargetCpu) {
    if let TargetCpu::Specific(cpu) = target {
        assert!(*cpu < 8, "affinity routing is disabled; cannot target a CPU with id >= 8");
    }
}

/// Initializes the distributor by enabling forwarding
/// of group 1 interrupts
///
/// Return value: whether or not affinity routing is
/// currently enabled for both secure and non-secure
/// states.
pub fn init(registers: &mut MmioPageOfU32) -> Enabled {
    let mut reg = registers[offset::CTLR];
    reg |= CTLR_ENGRP1;
    registers[offset::CTLR] = reg;

    // Return value: whether or not affinity routing is
    // currently enabled for both secure and non-secure
    // states.
    reg & CTLR_ARE_NS > 0
}

/// Will that interrupt be forwarded by the distributor?
pub fn get_int_state(registers: &MmioPageOfU32, int: IntNumber) -> Enabled {
    // enabled?
    read_array::<32>(registers, offset::ISENABLER, int) > 0
    &&
    // part of group 1?
    read_array::<32>(registers, offset::IGROUPR, int) == GROUP_1
}

/// Enables or disables the forwarding of
/// a particular interrupt in the distributor
pub fn set_int_state(registers: &mut MmioPageOfU32, int: IntNumber, enabled: Enabled) {
    let reg_base = match enabled {
        true  => offset::ISENABLER,
        false => offset::ICENABLER,
    };
    write_array::<32>(registers, reg_base, int, 1);

    // whether we're enabling or disabling,
    // set as part of group 1
    write_array::<32>(registers, reg_base, int, GROUP_1);
}

/// Sends an Inter-Processor-Interrupt
///
/// legacy / GICv2 method
/// int_num must be less than 16
pub fn send_ipi_gicv2(registers: &mut MmioPageOfU32, int_num: u32, target: TargetCpu) {
    assert_cpu_bounds(&target);

    let target_list = match target {
        TargetCpu::Specific(cpu) => (1 << cpu) << 16,
        TargetCpu::AnyCpuAvailable => SGIR_TARGET_ALL_OTHER_PE,
        TargetCpu::GICv2TargetList(list) => (list.bits as u32) << 16,
    };

    let value: u32 = int_num | target_list | SGIR_NSATT_GRP0;
    registers[offset::SGIR] = value;
}

impl super::ArmGic {
    pub(crate) fn distributor(&self) -> &MmioPageOfU32 {
        match self {
            Self::V2(v2) => &v2.distributor,
            Self::V3(v3) => &v3.distributor,
        }
    }

    pub(crate) fn distributor_mut(&mut self) -> &mut MmioPageOfU32 {
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
    pub fn get_spi_target(&self, int: IntNumber) -> TargetCpu {
        assert!(int >= 32, "interrupts number below 32 (SGIs & PPIs) don't have a target CPU");
        if !self.affinity_routing() {
            let flags = read_array::<4>(self.distributor(), offset::ITARGETSR, int);
            if flags == 0xff {
                return TargetCpu::AnyCpuAvailable;
            }

            for i in 0..8 {
                let target = 1 << i;
                if target & flags == target {
                    return TargetCpu::Specific(target);
                }
            }

            let list = TargetList::from_bits_truncate(flags as u8);
            TargetCpu::GICv2TargetList(list)
        } else if let Self::V3(v3) = self {
            let reg = v3.dist_extended[offset::P6IROUTER];

            // bit 31: Interrupt Routing Mode
            // value of 1 to target any available cpu
            // value of 0 to target a specific cpu
            if reg & (1 << 31) > 0 {
                TargetCpu::AnyCpuAvailable
            } else {
                let aff3 = (reg >> 8) & 0xff000000;
                let aff012 = reg & 0xffffff;
                TargetCpu::Specific(aff3 | aff012)
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
    pub fn set_spi_target(&mut self, int: IntNumber, target: TargetCpu) {
        assert!(int >= 32, "interrupts number below 32 (SGIs & PPIs) don't have a target CPU");
        if !self.affinity_routing() {
            assert_cpu_bounds(&target);

            let value = match target {
                TargetCpu::Specific(cpu) => 1 << cpu,
                TargetCpu::AnyCpuAvailable => 0xff,
                TargetCpu::GICv2TargetList(list) => list.bits as u32,
            };

            write_array::<4>(self.distributor_mut(), offset::ITARGETSR, int, value);
        } else if let Self::V3(v3) = self {
            let value = match target {
                TargetCpu::Specific(cpu) => {
                    // shift aff3 8 bits to the left
                    let aff3 = (cpu & 0xff000000) << 8;
                    // keep aff 0, 1 & 2 where they are
                    let aff012 = cpu & 0xffffff;
                    // leave bit 31 clear to indicate
                    // a specific target
                    aff3 | aff012
                },
                // bit 31: Interrupt Routing Mode
                // value of 1 to target any available cpu
                TargetCpu::AnyCpuAvailable => 1 << 31,
                TargetCpu::GICv2TargetList(_) => {
                    panic!("Cannot use TargetCpu::GICv2TargetList with GICv3!");
                },
            };

            v3.dist_extended[offset::P6IROUTER] = value;
        }

        // If we're on gicv2 then affinity routing is off
        // so we landed in the first block
        unreachable!()
    }
}
