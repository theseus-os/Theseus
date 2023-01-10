use super::MmioPageOfU32;
use super::U32BYTES;
use super::IntNumber;
use super::Enabled;
use super::read_array;
use super::write_array;

mod offset {
    use super::U32BYTES;
    pub const RD_WAKER: usize = 0x14 / U32BYTES;
    pub const SGI_ISENABLER: usize = 0x100 / U32BYTES;
    pub const SGI_ICENABLER: usize = 0x180 / U32BYTES;
}

const RD_WAKER_PROCESSOR_SLEEP: u32 = 1 << 1;
// const RD_WAKER_CHLIDREN_ASLEEP: u32 = 1 << 2;

pub fn init(registers: &mut MmioPageOfU32) {
    let mut reg;
    reg = registers[offset::RD_WAKER];
    // Wake the redistributor
    reg &= !RD_WAKER_PROCESSOR_SLEEP;
    registers[offset::RD_WAKER] = reg;

    // then poll ChildrenAsleep until it's cleared

    /* TODO
    // this could lock the entire kernel...
    // but it's what we're supposed to do
    // maybe we should put a loop run limit?

    // Another problem with this: it loops forever
    // I don't understand why yet, could this be the result
    // of non-volatile access to mmio?

    // The redist interface of core #0 is
    // already enabled when is code is reached,
    // so this has no implication for the moment

    let children_asleep = || {
        registers[offset::RD_WAKER] & RD_WAKER_CHLIDREN_ASLEEP > 0
    };
    log::info!("before while(children_asleep)");
    // while children_asleep() {}
    log::info!("after while(children_asleep)");

    */
}

/// Will that interrupt be forwarded by the GIC?
pub fn get_int_state(registers: &MmioPageOfU32, int: IntNumber) -> Enabled {
    read_array::<32>(registers, offset::SGI_ISENABLER, int) > 0
}

/// Enables or disables the forwarding of
/// a particular interrupt
pub fn set_int_state(registers: &mut MmioPageOfU32, int: IntNumber, enabled: Enabled) {
    let offset = match enabled {
        true => offset::SGI_ISENABLER,
        false => offset::SGI_ICENABLER,
    };
    write_array::<32>(registers, offset, int, 1);
}
