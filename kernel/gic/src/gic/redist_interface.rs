//! Redistributor Interface
//!
//! The Redistributor forwards or discards PPIs (private peripheral interrupts)
//! & SGIs (software generated interrupts) to the CPU core, in GICv3. There's
//! one redistributor per CPU core.
//!
//! Included functionality:
//! - Initializing the interface
//! - Enabling or disabling the forwarding of PPIs & SGIs based on their numbers
//! - Getting or setting the priority of PPIs & SGIs based on their numbers

use super::GicRegisters;
use super::InterruptNumber;
use super::Enabled;
use super::Priority;

mod offset {
    use crate::{Offset32, Offset64};
    pub(crate) const CTLR:              Offset32 = Offset32::from_byte_offset(0x00);
    pub(crate) const TYPER:             Offset64 = Offset64::from_byte_offset(0x08);
    pub(crate) const WAKER:             Offset32 = Offset32::from_byte_offset(0x14);
    pub(crate) const IGROUPR:           Offset32 = Offset32::from_byte_offset(0x80);
    pub(crate) const SGIPPI_ISENABLER:  Offset32 = Offset32::from_byte_offset(0x100);
    pub(crate) const SGIPPI_ICENABLER:  Offset32 = Offset32::from_byte_offset(0x180);
    pub(crate) const SGIPPI_IPRIORITYR: Offset32 = Offset32::from_byte_offset(0x400);
}

const WAKER_PROCESSOR_SLEEP: u32 = 1 << 1;
const WAKER_CHLIDREN_ASLEEP: u32 = 1 << 2;

/// Bit that is set if GICR_CTLR.DPG* bits are supported
const TYPER_DPGS: u64 = 1 << 5;

/// If bit is set, the PE cannot be selected for non-secure group 1 "1 of N" interrupts.
const CTLR_DPG1S: u32 = 1 << 26;

/// If bit is set, the PE cannot be selected for secure group 1 "1 of N" interrupts.
const CTLR_DPG1NS: u32 = 1 << 25;

/// If bit is set, the PE cannot be selected for group 0 "1 of N" interrupts.
const CTLR_DPG0: u32 = 1 << 24;

/// const GROUP_0: u32 = 0;
const GROUP_1: u32 = 1;

/// This timeout value works on some ARM SoCs:
/// - qemu's virt virtual machine
///
/// (if the value works for your SoC, please add it to this list.)
///
/// If the redistributor's initialization times out, it means either:
/// - that your ARM SoC is not GICv3 compliant (try initializing it as GICv2)
/// - that the timeout value is too low for your ARM SoC. Try increasing it
/// to see if the booting sequence continues.
///
/// If it wasn't enough for your machine, reach out to the Theseus
/// developers (or directly submit a PR).
const TIMEOUT_ITERATIONS: usize = 0xffff;

/// Initializes the redistributor by waking it up and waiting for it to awaken.
///
/// Returns an error if a timeout occurs while waiting.
pub fn init(registers: &mut GicRegisters) -> Result<(), &'static str> {
    let mut reg = registers.read_volatile(offset::WAKER);

    // Wake the redistributor
    reg &= !WAKER_PROCESSOR_SLEEP;
    registers.write_volatile(offset::WAKER, reg);

    // Then, wait for the children to wake up, timing out if it never happens.
    let children_asleep = || {
        registers.read_volatile(offset::WAKER) & WAKER_CHLIDREN_ASLEEP > 0
    };
    let mut counter = 0;
    while children_asleep() {
        counter += 1;
        if counter >= TIMEOUT_ITERATIONS {
            break;
        }
    }

    if counter >= TIMEOUT_ITERATIONS {
        return Err("BUG: gic driver: The redistributor didn't wake up in time.");
    }

    if registers.read_volatile_64(offset::TYPER) & TYPER_DPGS != 0 {
        // DPGS bits are supported in GICR_CTLR
        let mut reg = registers.read_volatile(offset::CTLR);

        // Enable PE selection for non-secure group 1 SPIs
        reg &= !CTLR_DPG1NS;

        // Disable PE selection for group 0 & secure group 1 SPIs
        reg |= CTLR_DPG0;
        reg |= CTLR_DPG1S;

        registers.write_volatile(offset::CTLR, reg);
    }

    Ok(())
}

/// Returns whether the given SGI (software generated interrupts) or
/// PPI (private peripheral interrupts) will be forwarded by the redistributor
pub fn is_sgippi_enabled(registers: &GicRegisters, int: InterruptNumber) -> Enabled {
    registers.read_array_volatile::<32>(offset::SGIPPI_ISENABLER, int) > 0
    &&
    // part of group 1?
    registers.read_array_volatile::<32>(offset::IGROUPR, int) == GROUP_1
}

/// Enables or disables the forwarding of a particular
/// SGI (software generated interrupts) or PPI (private
/// peripheral interrupts)
pub fn enable_sgippi(registers: &mut GicRegisters, int: InterruptNumber, enabled: Enabled) {
    let reg = match enabled {
        true => offset::SGIPPI_ISENABLER,
        false => offset::SGIPPI_ICENABLER,
    };
    registers.write_array_volatile::<32>(reg, int, 1);

    // whether we're enabling or disabling,
    // set as part of group 1
    registers.write_array_volatile::<32>(offset::IGROUPR, int, GROUP_1);
}

/// Returns the priority of an SGI/PPI.
pub fn get_sgippi_priority(registers: &GicRegisters, int: InterruptNumber) -> Priority {
    u8::MAX - (registers.read_array_volatile::<4>(offset::SGIPPI_IPRIORITYR, int) as u8)
}

/// Sets the priority of an SGI/PPI.
pub fn set_sgippi_priority(registers: &mut GicRegisters, int: InterruptNumber, prio: Priority) {
    registers.write_array_volatile::<4>(offset::SGIPPI_IPRIORITYR, int, (u8::MAX - prio) as u32);
}

/// Returns the internal ID of the redistributor
///
/// Note: this is only provided for debugging purposes
pub fn get_internal_id(registers: &GicRegisters) -> u16 {
    (registers.read_volatile_64(offset::TYPER) >> 8) as _
}
