use super::MmioPageOfU32;
use super::U32BYTES;
use super::IntNumber;
use super::Enabled;
use super::read_array;
use super::write_array;

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

/// Initializes the redistributor by waking
/// it up and checking that it's awake
pub fn init(registers: &mut MmioPageOfU32) {
    let mut reg;
    reg = registers[offset::RD_WAKER];
    // Wake the redistributor
    reg &= !RD_WAKER_PROCESSOR_SLEEP;
    registers[offset::RD_WAKER] = reg;

    // then poll ChildrenAsleep until it's cleared

    let children_asleep = || {
        let ptr = &registers[offset::RD_WAKER] as *const u32;
        let value = unsafe { ptr.read_volatile() };
        value & RD_WAKER_CHLIDREN_ASLEEP > 0
    };
    while children_asleep() {}
}

/// Will that SGI or PPI be forwarded by the GIC?
pub fn get_sgippi_state(registers: &MmioPageOfU32, int: IntNumber) -> Enabled {
    read_array::<32>(registers, offset::SGI_ISENABLER, int) > 0
    &&
    // part of group 1?
    read_array::<32>(registers, offset::IGROUPR, int) == GROUP_1
}

/// Enables or disables the forwarding of
/// a particular SGI or PPI
pub fn set_sgippi_state(registers: &mut MmioPageOfU32, int: IntNumber, enabled: Enabled) {
    let reg = match enabled {
        true => offset::SGI_ISENABLER,
        false => offset::SGI_ICENABLER,
    };
    write_array::<32>(registers, reg, int, 1);

    // whether we're enabling or disabling,
    // set as part of group 1
    write_array::<32>(registers, offset::IGROUPR, int, GROUP_1);
}
