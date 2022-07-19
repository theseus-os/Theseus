//! This crate contains abstractions for the x86 HPET.
//!
//! The actual HPET instance is defined in [`hpet-acpi`] as it must be
//! initialised from the ACPI tables.

#![no_std]

use core::sync::atomic::{AtomicU64, Ordering};
use hpet_acpi::{hpet, hpet_mut};
use log::debug;
use time::Duration;

// const IRQ_NUM: u8 = 0x10;
// const INTERRUPT_NUM: u8 = interrupts::IRQ_BASE_OFFSET + IRQ_NUM;

/// The number of times the [`HPET`]'s main counter has overflowed.
static HPET_OVERFLOWS: AtomicU64 = AtomicU64::new(0);

pub struct HpetClock;

impl time::Clock for HpetClock {
    type ClockType = time::Monotonic;

    /// This function will always return `false` if called prior to the parsing
    /// of ACPI tables.
    fn exists() -> bool {
        hpet().is_some()
    }

    /// Initialised the HPET.
    fn init() -> Result<(), &'static str> {
        let mut hpet = match hpet_mut() {
            Some(hpet) => hpet,
            None => return Err("HPET doesn't exist"),
        };

        // hpet.general_configuration.update(|value| {
        //     // Clear bit 1 (disable legacy mapping)
        //     *value &= !(1 << 1);
        // });

        // // TODO: Document timer 0 is being used by OS.
        // // The HPET is guaranteed to have at least three timers.
        // let overflow_timer = &mut hpet.timers[0];
        // // TODO: From OS Dev Wiki: "If the timer is set to 32 bit mode, it will also
        // generate an // interrupt when the counter wraps around." Will this
        // trigger a double interrupt?

        // let routing_capabilities = overflow_timer.configuration_and_capability.read()
        // >> 32u32; let mut io_apic_line: u8 = 32;
        // // let mut io_apic_lines = [false; 32];
        // for i in 0..32 {
        //     // if the ith bit is set.
        //     if ((routing_capabilities >> i) & 0x1) == 1 {
        //         io_apic_line = i;
        //         break;
        //         // io_apic_lines[i] = true;
        //     }
        // }
        // // FIXME: Check for the intersection between unused I/O APIC lines and
        // io_apic_lines. if io_apic_line == 32 {
        //     return Err("Couldn't find suitable I/O APIC line for HPET");
        // }

        // // FIXME: Which I/O APIC number?
        // ioapic::get_ioapic(0)
        //     .ok_or("couldn't get I/O APIC")?
        //     .set_irq(io_apic_line, 0, INTERRUPT_NUM);

        // overflow_timer.configuration_and_capability.update(|value| {
        //     // Clear bit 14 (use standard interrupt mapping)
        //     // *value &= !(1 << 14);
        //     // Write to bytes 9-13 (I/O APIC line to use)
        //     // TODO: I'm not sure if clearing the bits first is necessary.
        //     // for i in 9..=13 {
        //     //     *value &= !(1 << i);
        //     // }
        //     // io_apic_line is guaranteed to be <= 31 and so it won't overwrite more
        // than five     // bytes.
        //     *value |= (io_apic_line as u64) << 9;
        //     // Set bit 8 (force 32-bit mode)
        //     // TODO: Alternatively we can read bit 5 and account for whether timer is
        // 32 or     // 64-bit.
        //     // *value |= 1 << 8;
        //     // Clear bit 3 (enable non-periodic mode)
        //     // *value &= !(1 << 3);
        //     *value |= 1 << 3;
        //     // FIXME: Tn_INT_TYPE_CNF
        //     // Clear bit 1
        //     // *value &= !(1 << 1);
        //     // Set bit 2 (enable interrupts)
        //     *value |= 1 << 2;
        // });

        // overflow_timer.comparator_value.write(0_000_000_000u64);
        // // overflow_timer.comparator_value.write(1_000_000_000u64);

        // interrupts::register_interrupt(INTERRUPT_NUM, hpet_overflow_handler)
        //     .map_err(|_| "0x30 interrupt number already in use")?;

        // TODO
        if !hpet.is_64_bit() {
            return Err("Main counter isn't 64 bit");
        }

        hpet.enable_counter(true);
        debug!(
            "Initialized HPET, period: {}, counter val: {}, num timers: {}, vendor_id: {}",
            hpet.counter_period_femtoseconds(),
            hpet.counter(),
            hpet.num_timers(),
            hpet.vendor_id()
        );
        Ok(())
    }

    fn now() -> Duration {
        const FEMTOS_PER_NANO: u32 = 1_000_000;
        let hpet = hpet().expect("HPET does not exist");

        let counter_value = hpet.counter();
        let overflows = HPET_OVERFLOWS.load(Ordering::SeqCst);

        let counter_period_femtoseconds = hpet.counter_period_femtoseconds() as u64;
        let counter_period_nanoseconds = counter_period_femtoseconds / (FEMTOS_PER_NANO as u64);

        let nanos_per_overflow: u64 = u32::MAX as u64 * counter_period_nanoseconds as u64;

        let nanos = (counter_value * counter_period_nanoseconds) + (overflows * nanos_per_overflow);
        Duration::from_nanos(nanos)
    }
}

impl time::EarlySleeper for HpetClock {
    const INIT_REQUIRED: bool = true;
}

// extern "x86-interrupt" fn hpet_overflow_handler(_:
// interrupts::InterruptStackFrame) {     // hpet_mut().unwrap().
// general_interrupt_status.update(|value| {     //     *value |= 1;
//     // });
//     HPET_OVERFLOWS.fetch_add(1, Ordering::SeqCst);
//     interrupts::eoi(Some(INTERRUPT_NUM));
// }
