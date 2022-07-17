//! This crate contains abstractions for the x86 PIT.

#![no_std]

use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use kernel_config::time::CONFIG_PIT_FREQUENCY_HZ;
use port_io::Port;
use spin::Mutex;
use time::Duration;

// The timer's default frequency in hertz.
const PIT_DEFAULT_FREQUENCY: u32 = 1193182;
/// THE timer's minimum frequency in hertz.
const PIT_MINIMUM_FREQUENCY: u32 = 19;

const CHANNEL_0_PORT: u16 = 0x40;
const CHANNEL_2_PORT: u16 = 0x42;
const COMMAND_REGISTER_PORT: u16 = 0x43;

const NANOS_IN_SEC: u32 = 1_000_000_000;

static CHANNEL_0: Mutex<Port<u8>> = Mutex::new(Port::new(CHANNEL_0_PORT));
static CHANNEL_2: Mutex<Port<u8>> = Mutex::new(Port::new(CHANNEL_2_PORT));
static COMMAND_REGISTER: Mutex<Port<u8>> = Mutex::new(Port::new(COMMAND_REGISTER_PORT));

static PIT_TICKS: AtomicU64 = AtomicU64::new(0);
/// The period between PIT interrupts in nanoseconds.
// u32::max ns = 4.3 s
// 1 / PIT_MINIMUM_FREQUENCY = 0.05 s
// 4.3 > 0.05 therefore a u32 is large enough
static PIT_PERIOD: AtomicU32 = AtomicU32::new(0);

pub struct PitClock;

impl time::Clock for PitClock {
    type ClockType = time::Monotonic;

    fn exists() -> bool {
        // FIXME
        true
    }

    fn init() -> Result<(), &'static str> {
        let divisor = PIT_DEFAULT_FREQUENCY / CONFIG_PIT_FREQUENCY_HZ;
        // TODO: const assert
        assert!(divisor > (u16::max_value() as u32));

        PIT_PERIOD.store(NANOS_IN_SEC / CONFIG_PIT_FREQUENCY_HZ, Ordering::SeqCst);

        // SAFE because we're simply configuring the PIT clock, and the code below is
        // correct.
        unsafe {
            COMMAND_REGISTER.lock().write(0x36); // 0x36: see this: http://www.osdever.net/bkerndev/Docs/pit.htm

            // must write the low byte and then the high byte
            CHANNEL_0.lock().write(divisor as u8);
            // read from PS/2 port 0x60, which acts as a short delay and acknowledges the
            // status register
            let _ignore: u8 = Port::new(0x60).read();
            CHANNEL_0.lock().write((divisor >> 8) as u8);
        }

        // interrupts::register_interrupt(interrupts::IRQ_BASE_OFFSET, pit_interrupt)
        //     .map_err(|_| "failed to register pit interrupt handler")?;
        Ok(())
    }

    fn now() -> Duration {
        Duration::from_nanos(
            PIT_TICKS.load(Ordering::SeqCst) as u64 * PIT_PERIOD.load(Ordering::SeqCst) as u64,
        )
    }
}

impl time::EarlySleeper for PitClock {
    // FIXME: Is this right?
    const INIT_REQUIRED: bool = false;

    /// Sleep for the given `duration`.
    ///
    /// This implementation does not rely on interrupts.
    // TODO: Does init need to be called beforehand?
    fn sleep(duration: Duration) {
        const MAX_SLEEP: u32 = NANOS_IN_SEC / PIT_MINIMUM_FREQUENCY;

        let mut nanos = duration.as_nanos();

        // TODO: Test and cleanup comments.
        loop {
            let sleep_nanos = core::cmp::min(nanos, MAX_SLEEP as u128);
            let divisor = PIT_DEFAULT_FREQUENCY as u128 / (NANOS_IN_SEC as u128 / sleep_nanos);

            let port_60 = Port::<u8>::new(0x60);
            let port_61 = Port::<u8>::new(0x61);

            unsafe {
                // TODO: Is this really supposed to be a link to the APIC timer page.
                // see code example: https://wiki.osdev.org/APIC_timer
                let port_61_val = port_61.read();
                port_61.write(port_61_val & 0xFD | 0x1); // sets the speaker channel 2 to be controlled by PIT hardware
                COMMAND_REGISTER.lock().write(0b10110010); // channel 2, access mode: lobyte/hibyte, hardware-retriggerable one shot mode,
                                                           // 16-bit binary (not BCD)

                // set frequency; must write the low byte first and then the high byte
                CHANNEL_2.lock().write(divisor as u8);
                // read from PS/2 port 0x60, which acts as a short delay and acknowledges the
                // status register
                let _ignore: u8 = port_60.read();
                CHANNEL_2.lock().write((divisor >> 8) as u8);

                // reset PIT one-shot counter
                let port_61_val = port_61.read() & 0xFE;
                port_61.write(port_61_val); // clear bit 0
                port_61.write(port_61_val | 0x1); // set bit 0
                                                  // here, PIT channel 2 timer has started counting
                                                  // here, should also run custom reset function (closure input), e.g., resetting
                                                  // APIC counter

                // wait for PIT timer to reach 0, which is tested by checking bit 5
                while port_61.read() & 0x20 != 0 {}
            }

            if sleep_nanos < MAX_SLEEP as u128 {
                return;
            } else {
                nanos -= sleep_nanos
            }
        }
    }
}
