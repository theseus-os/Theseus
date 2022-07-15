use crate::Duration;
use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use kernel_config::time::CONFIG_PIT_FREQUENCY_HZ;
use log::{error, trace};
use port_io::Port;
use spin::Mutex;

const NANOS_IN_SEC: u32 = 1_000_000_000;

/// The main interrupt channel
const CHANNEL0: u16 = 0x40;
/// DO NOT USE
const _CHANNEL1: u16 = 0x41;
/// The channel used for one-shot timers.
const CHANNEL2: u16 = 0x42;

const COMMAND_REGISTER: u16 = 0x43;

/// the timer's default frequency is 1.19 MHz
const PIT_DEFAULT_DIVIDEND_HZ: u32 = 1193182;
const PIT_MINIMUM_FREQ: u32 = 19;

static PIT_COMMAND: Mutex<Port<u8>> = Mutex::new(Port::new(COMMAND_REGISTER));
static PIT_CHANNEL_0: Mutex<Port<u8>> = Mutex::new(Port::new(CHANNEL0));
static PIT_CHANNEL_2: Mutex<Port<u8>> = Mutex::new(Port::new(CHANNEL2));

static PIT_TICKS: AtomicUsize = AtomicUsize::new(0);

// The period between PIT interrupts in nanoseconds.
static PIT_PERIOD: AtomicU64 = AtomicU64::new(0);

pub(crate) fn exists() -> bool {
    // TODO
    true
}

pub(crate) fn init() -> Result<(), &'static str> {
    let divisor = PIT_DEFAULT_DIVIDEND_HZ / CONFIG_PIT_FREQUENCY_HZ;
    if divisor > (u16::max_value() as u32) {
        panic!(
            "The chosen PIT frequency ({} Hz) is too small, it must be {} Hz or greater!",
            CONFIG_PIT_FREQUENCY_HZ, PIT_MINIMUM_FREQ
        );
    }

    PIT_PERIOD.store(
        NANOS_IN_SEC as u64 / CONFIG_PIT_FREQUENCY_HZ as u64,
        Ordering::SeqCst,
    );

    // SAFE because we're simply configuring the PIT clock, and the code below is correct.
    unsafe {
        PIT_COMMAND.lock().write(0x36); // 0x36: see this: http://www.osdever.net/bkerndev/Docs/pit.htm

        // must write the low byte and then the high byte
        PIT_CHANNEL_0.lock().write(divisor as u8);
        // read from PS/2 port 0x60, which acts as a short delay and acknowledges the status register
        let _ignore: u8 = Port::new(0x60).read();
        PIT_CHANNEL_0.lock().write((divisor >> 8) as u8);
    }

    interrupts::register_interrupt(interrupts::IRQ_BASE_OFFSET, pit_interrupt)
        .map_err(|_| "failed to register pit interrupt handler")?;
    Ok(())
}

/// Wait for the given duration.
///
/// The duration must be no greater than 1 / [`PIT_MINIMUM_FREQ`] seconds.
/// Uses a separate PIT clock channel, so it doesn't affect the regular PIT interrupts on PIT channel 0.
pub(crate) fn wait(duration: Duration) -> Result<(), &'static str> {
    if duration.as_nanos() > NANOS_IN_SEC as u128 / PIT_MINIMUM_FREQ as u128 {
        error!(
            "time::pit::wait(): the chosen wait time {} ns is too large, max value is {} ns",
            duration.as_nanos(),
            NANOS_IN_SEC / PIT_MINIMUM_FREQ
        );
        return Err("chosen wait time too large");
    }
    let divisor = PIT_DEFAULT_DIVIDEND_HZ as u128 / (NANOS_IN_SEC as u128 / duration.as_nanos());

    let port_60 = Port::<u8>::new(0x60);
    let port_61 = Port::<u8>::new(0x61);

    unsafe {
        // TODO: Is this really supposed to be a link to the APIC timer page.
        // see code example: https://wiki.osdev.org/APIC_timer
        let port_61_val = port_61.read();
        port_61.write(port_61_val & 0xFD | 0x1); // sets the speaker channel 2 to be controlled by PIT hardware
        PIT_COMMAND.lock().write(0b10110010); // channel 2, access mode: lobyte/hibyte, hardware-retriggerable one shot mode, 16-bit binary (not BCD)

        // set frequency; must write the low byte first and then the high byte
        PIT_CHANNEL_2.lock().write(divisor as u8);
        // read from PS/2 port 0x60, which acts as a short delay and acknowledges the status register
        let _ignore: u8 = port_60.read();
        PIT_CHANNEL_2.lock().write((divisor >> 8) as u8);

        // reset PIT one-shot counter
        let port_61_val = port_61.read() & 0xFE;
        port_61.write(port_61_val); // clear bit 0
        port_61.write(port_61_val | 0x1); // set bit 0
                                          // here, PIT channel 2 timer has started counting
                                          // here, should also run custom reset function (closure input), e.g., resetting APIC counter

        // wait for PIT timer to reach 0, which is tested by checking bit 5
        while port_61.read() & 0x20 != 0 {}
    }

    Ok(())
}

pub(crate) fn now() -> Duration {
    Duration::from_nanos(
        PIT_TICKS.load(Ordering::SeqCst) as u64 * PIT_PERIOD.load(Ordering::SeqCst) as u64,
    )
}

// pub(crate) fn wait(Duration)

extern "x86-interrupt" fn pit_interrupt(_: interrupts::InterruptStackFrame) {
    let ticks = PIT_TICKS.fetch_add(1, Ordering::SeqCst);
    trace!("PIT timer interrupt, ticks: {}", ticks);
}
