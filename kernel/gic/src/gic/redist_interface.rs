//! Redistributor Interface
//!
//! The Redistributor forwards or discards PPIs (private peripheral interrupts)
//! & SGIs (software generated interrupts) to the CPU core, in GICv3. There's
//! one redistributor per CPU core.
//!
//! Included functionality:
//! - Initializing the interface
//! - Enabling or disabling the forwarding of PPIs & SGIs based on their numbers

use super::GicRegisters;
use super::InterruptNumber;
use super::Enabled;

mod offset {
    use crate::Offset32;
    pub(crate) const RD_WAKER: Offset32 = Offset32::from_byte_offset(0x14);
    pub(crate) const IGROUPR:  Offset32 = Offset32::from_byte_offset(0x80);
    pub(crate) const SGIPPI_ISENABLER: Offset32 = Offset32::from_byte_offset(0x100);
    pub(crate) const SGIPPI_ICENABLER: Offset32 = Offset32::from_byte_offset(0x180);
}

const RD_WAKER_PROCESSOR_SLEEP: u32 = 1 << 1;
const RD_WAKER_CHLIDREN_ASLEEP: u32 = 1 << 2;

// const GROUP_0: u32 = 0;
const GROUP_1: u32 = 1;

// This timeout value works on some ARM SoCs:
// - qemu's virt virtual machine
//
// (if the value works for your SoC, please add it to this list.)
//
// If the redistributor's initialization times out, it means either:
// - that your ARM SoC is not GICv3 compliant (try initializing it as GICv2)
// - that the timeout value is too low for your ARM SoC. Try increasing it
// to see if the booting sequence continues.
//
// If it wasn't enough for your machine, reach out to the Theseus
// developers (or directly submit a PR).
const TIMEOUT_ITERATIONS: usize = 0xffff;

/// Initializes the redistributor by waking
/// it up and checking that it's awake
pub fn init(registers: &mut GicRegisters) -> Result<(), &'static str> {
    let mut reg;
    reg = registers.read_volatile(offset::RD_WAKER);

    // Wake the redistributor
    reg &= !RD_WAKER_PROCESSOR_SLEEP;

    registers.write_volatile(offset::RD_WAKER, reg);

    // then poll ChildrenAsleep until it's cleared

    let children_asleep = || {
        registers.read_volatile(offset::RD_WAKER) & RD_WAKER_CHLIDREN_ASLEEP > 0
    };

    let mut counter = 0;
    let mut timed_out = || {
        counter += 1;
        counter >= TIMEOUT_ITERATIONS
    };

    while children_asleep() && !timed_out() { }

    match timed_out() {
        false => Ok(()),

        // see definition of TIMEOUT_ITERATIONS
        true => Err("gic: The redistributor didn't wake up in time."),
    }
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
