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

use super::InterruptNumber;
use super::Enabled;
use super::Priority;
use super::read_array_volatile;
use super::write_array_volatile;

use volatile::{Volatile, ReadOnly};
use zerocopy::FromBytes;

/// General redistributor registers
#[derive(FromBytes)]
#[repr(C)]
pub struct RedistRegsP1 {                          // base offset
    /// Redistributor Control Register
    ctlr:         Volatile<u32>,                   // 0x00

    /// Implementer Identification Register
    _unused0:     u32,

    /// Redistributor Type Register
    ident:        ReadOnly<u64>,                   // 0x08

    /// Error Reporting Status Register, optional
    _unused1:     u32,

    /// Redistributor Wake Register
    waker:        Volatile<u32>,                   // 0x14
}

/// Redistributor registers for SGIs & PPIs
#[derive(FromBytes)]
#[repr(C)]
pub struct RedistRegsSgiPpi {            // base offset
    _reserved0:   [u8;            0x80],

    /// Interrupt Group Register 0
    group:        [Volatile<u32>; 0x01], // 0x080
    _reserved1:   [u32;           0x1f],

    /// Interrupt Set-Enable Registers
    set_enable:   [Volatile<u32>; 0x01], // 0x100
    _reserved2:   [u32;           0x1f],

    /// Interrupt Clear-Enable Registers
    clear_enable: [Volatile<u32>; 0x01], // 0x180
    _reserved3:   [u32;           0x1f],

    /// Interrupt Set & Clear Pending / Active Registers
    _unused0:     [u32;           0x80],

    /// Interrupt Priority Registers
    priority:     [Volatile<u32>; 0x08], // 0x400
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

impl RedistRegsP1 {
    /// Initializes the redistributor by waking it up and waiting for it to awaken.
    ///
    /// Returns an error if a timeout occurs while waiting.
    pub fn init(&mut self) -> Result<(), &'static str> {
        let mut reg = self.waker.read();

        // Wake the redistributor
        reg &= !WAKER_PROCESSOR_SLEEP;
        self.waker.write(reg);

        // Then, wait for the children to wake up, timing out if it never happens.
        let children_asleep = || {
            self.waker.read() & WAKER_CHLIDREN_ASLEEP > 0
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

        if self.ident.read() & TYPER_DPGS != 0 {
            // DPGS bits are supported in GICR_CTLR
            let mut reg = self.ctlr.read();

            // Enable PE selection for non-secure group 1 SPIs
            reg &= !CTLR_DPG1NS;

            // Disable PE selection for group 0 & secure group 1 SPIs
            reg |= CTLR_DPG0;
            reg |= CTLR_DPG1S;

            self.ctlr.write(reg);
        }

        Ok(())
    }

    /// Returns the internal ID of the redistributor
    ///
    /// Note: this is only provided for debugging purposes
    pub fn get_internal_id(&self) -> u16 {
        (self.ident.read() >> 8) as _
    }
}

impl RedistRegsSgiPpi {
    /// Returns whether the given SGI (software generated interrupts) or
    /// PPI (private peripheral interrupts) will be forwarded by the redistributor
    pub fn is_sgippi_enabled(&self, int: InterruptNumber) -> Enabled {
        read_array_volatile::<32>(&self.set_enable, int) > 0
        &&
        // part of group 1?
        read_array_volatile::<32>(&self.group, int) == GROUP_1
    }

    /// Enables or disables the forwarding of a particular
    /// SGI (software generated interrupts) or PPI (private
    /// peripheral interrupts)
    pub fn enable_sgippi(&mut self, int: InterruptNumber, enabled: Enabled) {
        let reg = match enabled {
            true => &mut self.set_enable,
            false => &mut self.clear_enable,
        };
        write_array_volatile::<32>(reg, int, 1);

        // whether we're enabling or disabling,
        // set as part of group 1
        write_array_volatile::<32>(&mut self.group, int, GROUP_1);
    }

    /// Returns the priority of an SGI/PPI.
    pub fn get_sgippi_priority(&self, int: InterruptNumber) -> Priority {
        u8::MAX - (read_array_volatile::<4>(&self.priority, int) as u8)
    }

    /// Sets the priority of an SGI/PPI.
    pub fn set_sgippi_priority(&mut self, int: InterruptNumber, prio: Priority) {
        write_array_volatile::<4>(&mut self.priority, int, (u8::MAX - prio) as u32);
    }
}
