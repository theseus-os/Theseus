//! Redistributor Interface
//!
//! Included functionnality:
//! - Initializing the interface
//! - Enabling or disabling the forwarding of PPIs & SGIs based on their numbers

use super::MmioPageOfU32;
use super::U32BYTES;
use super::IntNumber;
use super::Enabled;
use super::read_array_volatile;
use super::write_array_volatile;
use super::read_volatile;
use super::write_volatile;

mod offset {
    use super::U32BYTES;
    pub const RD_WAKER: usize = 0x14 / U32BYTES;
    pub const IGROUPR:  usize = 0x80 / U32BYTES;
    pub const SGI_ISENABLER: usize = 0x100 / U32BYTES;
    pub const SGI_ICENABLER: usize = 0x180 / U32BYTES;
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
const TIMEOUT_ITERATIONS: usize = 0x10_000;

/// Initializes the redistributor by waking
/// it up and checking that it's awake
pub fn init(registers: &mut MmioPageOfU32) -> Result<(), &'static str> {
    let mut reg;
    reg = read_volatile(&registers[offset::RD_WAKER]);

    // Wake the redistributor
    reg &= !RD_WAKER_PROCESSOR_SLEEP;

    write_volatile(&mut registers[offset::RD_WAKER], reg);

    // then poll ChildrenAsleep until it's cleared

    let children_asleep = || {
        read_volatile(&registers[offset::RD_WAKER]) & RD_WAKER_CHLIDREN_ASLEEP > 0
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

/// Returns whether the given interrupt will be forwarded by the distributor
pub fn get_sgippi_state(registers: &MmioPageOfU32, int: IntNumber) -> Enabled {
    read_array_volatile::<32>(registers, offset::SGI_ISENABLER, int) > 0
    &&
    // part of group 1?
    read_array_volatile::<32>(registers, offset::IGROUPR, int) == GROUP_1
}

/// Enables or disables the forwarding of
/// a particular SGI or PPI
pub fn set_sgippi_state(registers: &mut MmioPageOfU32, int: IntNumber, enabled: Enabled) {
    let reg = match enabled {
        true => offset::SGI_ISENABLER,
        false => offset::SGI_ICENABLER,
    };
    write_array_volatile::<32>(registers, reg, int, 1);

    // whether we're enabling or disabling,
    // set as part of group 1
    write_array_volatile::<32>(registers, offset::IGROUPR, int, GROUP_1);
}
