use core::arch::asm;
use super::TargetCpu;
use super::Priority;
use super::IntNumber;

const SGIR_TARGET_ALL_OTHER_PE: usize = 1 << 40;
const IGRPEN_ENABLED: usize = 1;

pub fn init() {
    let mut icc_ctlr: usize;

    unsafe { asm!("mrs {}, ICC_CTLR_EL1", out(reg) icc_ctlr) };
    // clear bit 1 (EOIMode) so that eoi signals both
    // priority drop & interrupt deactivation
    icc_ctlr &= !0b10;
    unsafe { asm!("msr ICC_CTLR_EL1, {}", in(reg) icc_ctlr) };

    // Enable Group 0
    // bit 0 = group 0 enable
    // unsafe { asm!("msr ICC_IGRPEN0_EL1, {}", in(reg) IGRPEN_ENABLED) };

    // Enable Groupe 1 (non-secure)
    // bit 0 = group 1 (non-secure) enable
    unsafe { asm!("msr ICC_IGRPEN1_EL1, {}", in(reg) IGRPEN_ENABLED) };
}

/// Interrupts have a priority; if their priority
/// is lower or equal to this one, they're discarded
pub fn get_minimum_int_priority() -> Priority {
    let mut reg_value: usize;
    unsafe { asm!("mrs {}, ICC_PMR_EL1", out(reg) reg_value) };
    255 - (reg_value as u8)
}

/// Interrupts have a priority; if their priority
/// is lower or equal to this one, they're discarded
pub fn set_minimum_int_priority(priority: Priority) {
    let reg_value = (255 - priority) as usize;
    unsafe { asm!("msr ICC_PMR_EL1, {}", in(reg) reg_value) };
}

/// Performs priority drop for the specified interrupt
pub fn end_of_interrupt(int: IntNumber) {
    let reg_value = int as usize;
    unsafe { asm!("msr ICC_EOIR1_EL1, {}", in(reg) reg_value) };
}

/// Acknowledge the currently serviced interrupt
/// and fetches its number
pub fn acknowledge_int() -> (IntNumber, Priority) {
    let int_num: usize;
    let priority: usize;

    // Reading the interrupt number has the side effect
    // of acknowledging the interrupt.
    unsafe {
        asm!("mrs {}, ICC_IAR1_EL1", out(reg) int_num);
        asm!("mrs {}, ICC_RPR_EL1", out(reg) priority);
    }

    let int_num = int_num & 0xffffff;
    let priority = priority & 0xff;
    (int_num as IntNumber, priority as u8)
}

pub fn send_ipi(int_num: IntNumber, target: TargetCpu) {
    let mut value = match target {
        TargetCpu::Specific(cpu) => {
            let cpu = cpu as usize;
            // aff3 in bits [48:55]
            let aff3 = (cpu & 0xff000000) << 24;
            // aff2 in bits [32:39]
            let aff2 = cpu & 0xff0000 << 16;
            // aff1 in bits [16:23]
            let aff1 = cpu & 0xff00 << 8;
            // aff0 as a GICv2-style target list
            let aff0 = cpu & 0xff;
            let target_list = if aff0 >= 16 {
                log::error!("[GIC driver] cannot send an IPI to a core with Aff0 >= 16");
                // target_list = 0 -> this IPI will be discarded
                0
            } else {
                1 << aff0
            };
            aff3 | aff2 | aff1 | target_list
        },
        // bit 31: Interrupt Routing Mode
        // value of 1 to target any available cpu
        TargetCpu::AnyCpuAvailable => SGIR_TARGET_ALL_OTHER_PE,
        TargetCpu::GICv2TargetList(_) => {
            panic!("Cannot use TargetCpu::GICv2TargetList with GICv3!");
        },
    };

    value |= (int_num as usize) << 24;
    unsafe { asm!("msr ICC_SGI1R_EL1, {}", in(reg) value) };
}