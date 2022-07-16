//! This crate provides abstractions for the x86 RTC.

#![no_std]

use port_io::Port;
use spin::Mutex;
use time::Duration;

// CMOS port number used to select registers.
const CMOS_WRITE_PORT: u16 = 0x70;
// CMOS port number used to read register values or change settings.
const CMOS_READ_PORT: u16 = 0x71;

// CMOS port used to select registers.
static CMOS_WRITE: Mutex<Port<u8>> = Mutex::new(Port::new(CMOS_WRITE_PORT));
// CMOS port used to read register values.
static CMOS_READ: Mutex<Port<u8>> = Mutex::new(Port::new(CMOS_READ_PORT));

pub struct RtcClock;

impl time::Clock for RtcClock {
    type ClockType = time::Realtime;

    fn exists() -> bool {
        // FIXME
        true
    }

    /// This function does nothing as using the RTC as a monotonic time source doesn't require
    /// initialisation.
    fn init() -> Result<(), &'static str> {
        Ok(())
    }

    /// Time since 12:00am January 1st 1970 (i.e. Unix time).
    ///
    /// The algorithm is based on [IEEE 1003.1-2017 Base Definitions 4.16].
    ///
    /// [IEEE 1003.1-2017 Base Definitions 4.16]: https://pubs.opengroup.org/onlinepubs/9699919799.2018edition/
    fn now() -> Duration {
        RtcTime::now().into()
    }
}

/// A timestamp obtained from the real-time clock.
#[derive(Debug)]
pub struct RtcTime {
    // The second of the minute (0-59).
    pub seconds: u8,
    /// The minute of the hour (0-59).
    pub minutes: u8,
    /// The hour of the day (0-23).
    // TODO: Are we in 12 or 24 hour mode.
    pub hours: u8,
    /// The day of the month (1-31).
    pub days: u8,
    /// The month of the year (1-12).
    pub months: u8,
    /// The year of the century (0-99).
    pub years: u8,
}

impl RtcTime {
    // Reads and returns the current [`RtcTime`] from the RTC CMOS.
    fn now() -> Self {
        let seconds = read_register(0x00);
        let minutes = read_register(0x02);
        let hours = read_register(0x04);
        let days = read_register(0x07);
        let months = read_register(0x08);
        let years = read_register(0x09);

        RtcTime {
            seconds,
            minutes,
            hours,
            days,
            months,
            years,
        }
    }
}

impl From<RtcTime> for Duration {
    /// Time since 12:00am January 1st 1970 (i.e. Unix time).
    ///
    /// The algorithm is based on [IEEE 1003.1-2017 Base Definitions 4.16].
    ///
    /// [IEEE 1003.1-2017 Base Definitions 4.16]: https://pubs.opengroup.org/onlinepubs/9699919799.2018edition/
    fn from(time: RtcTime) -> Self {
        const MINUTE_MULTIPLIER: u64 = 60;
        const HOUR_MULTIPLIER: u64 = 60 * MINUTE_MULTIPLIER;
        const DAY_MULTIPLIER: u64 = 24 * HOUR_MULTIPLIER;
        // Non-leap year
        const YEAR_MULTIPLIER: u64 = 365 * DAY_MULTIPLIER;

        let mut secs = time.seconds as u64;
        secs += time.minutes as u64 * MINUTE_MULTIPLIER;
        secs += time.hours as u64 * HOUR_MULTIPLIER;

        const DAYS_IN_MONTH: [u16; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
        let mut days_in_year: u16 = DAYS_IN_MONTH[0..(time.months as usize - 1)].iter().sum();
        // time.days is guaranteed to be >= 1
        days_in_year += time.days as u16 - 1;
        // Leap years
        if time.months >= 3 && (time.years % 4 == 0) {
            days_in_year += 1;
        }
        secs += days_in_year as u64 * DAY_MULTIPLIER;
        secs += (time.years as u64 + 30) * YEAR_MULTIPLIER;

        // TODO: This assumes we are in the 21st century.
        let years_since_1900 = time.years as u64 + 100;

        // Adds a day every 4 years starting in 1973.
        secs += ((years_since_1900 - 69) / 4) * DAY_MULTIPLIER;
        // Subtracts a day back out every 100 years starting in 2001.
        secs += ((years_since_1900 - 1) / 100) * DAY_MULTIPLIER;
        // Adds a day back in every 400 years starting in 2001.
        secs -= ((years_since_1900 + 299) / 400) * DAY_MULTIPLIER;

        Duration::from_secs(secs)
    }
}

// Write a [`u8`] to the CMOS port 0x70.
fn write_cmos(value: u8) {
    unsafe { CMOS_WRITE.lock().write(value) }
}

// Read a [`u8`] from the CMOS port 0x71.
fn read_cmos() -> u8 {
    CMOS_READ.lock().read()
}

// Returns true if an update is in progress, false otherwise.
fn is_update_in_progress() -> bool {
    // Writing to this register causes the CMOS to output 1 if an update is in progress.
    write_cmos(0x0A);
    let is_in_progress: bool = read_cmos() == 1;
    is_in_progress
}

// Read the given `register` of the RTC CMOS.
fn read_register(register: u8) -> u8 {
    // Wait for the "update in progress" signal to finish in order to read correct values.
    while is_update_in_progress() {}
    write_cmos(register);

    let bcd = read_cmos();

    // Convert the bcd value to binary.
    (bcd / 16) * 10 + (bcd & 0xf)
}

// TODO: The following code is for RTC interrupts, which I'm not sure we need.

// // CMOS port used to change settings.
// static CMOS_WRITE_SETTINGS: Mutex<Port<u8>> = Mutex::new(Port::new(CMOS_READ_PORT));

// type RtcTicks = AtomicUsize;
// lazy_static! {
//     static ref RTC_TICKS: SSCached<RtcTicks> = {
//         insert_state(RtcTicks::new(0));
//         get_state::<RtcTicks>()
//     };
// }

// pub type RtcInterruptFunction = fn(Option<usize>);

// static RTC_INTERRUPT_FUNC: Once<RtcInterruptFunction> = Once::new();

// /// Initialize the RTC interrupt with the given frequency
// /// and the given closure that will run on each RTC interrupt.
// /// The closure is provided with the current number of RTC ticks since boot,
// /// in the form of an `Option<usize>` because it is not guaranteed that the number of ticks can be retrieved.
// pub fn init(rtc_freq: usize, interrupt_func: RtcInterruptFunction) -> Result<(HandlerFunc), ()> {
//     RTC_INTERRUPT_FUNC.call_once(|| interrupt_func);
//     enable_rtc_interrupt();
//     let res = set_rtc_frequency(rtc_freq);
//     res.map( |_| rtc_interrupt_handler as HandlerFunc )
// }

// pub fn rtc_ticks() -> Result<usize, ()> {
//      if let Some(ticks) = RTC_TICKS.get() {
//          Ok(ticks.load(Ordering::Acquire))
//      }
//      else {
//         Err(())
//      }
// }

// /// Turn on IRQ 8 (mapped to 0x28), rtc begins sending interrupts
// pub fn enable_rtc_interrupt() {
//     let _held_interrupts = hold_interrupts();

//     write_cmos(0x0C);
//     read_cmos();
//     //select cmos register 0x8B
//     write_cmos(0x8B);

//     //value needed to turn on bit 6 of register B
//     let prev = read_cmos();

//     //we want it to go back to register 0x8B, it was reset when read
//     write_cmos(0x8B);

//     //here we don't use the cmos_write function because that only writes to port 0x70, in this case we need to write to 0x71
//     //writing to 0x71 because not selecting register, setting rtc
//     unsafe {
//         CMOS_WRITE_SETTINGS.lock().write(prev | 0x40);
//     }

//     trace!("RTC interrupts enabled");
//     // here: _held_interrupts falls out of scope, re-enabling interrupts if they were previously enabled.
// }

// /// the log base 2 of an integer value
// fn log2(value: usize) -> usize {
//     let mut v = value;
//     let mut result = 0;
//     v >>= 1;
//     while v > 0 {
//         result += 1;
//         v >>= 1;
//     }

//     result
// }

// /// Sets the RTC interrupt frequency.
// ///
// /// `rate` must be a power of 2, between 2 and 8192 inclusive.
// pub fn set_rtc_frequency(rate: usize) -> Result<(), ()> {
//     if !(rate.is_power_of_two() && rate >= 2 && rate <= 8192) {
//         error!(
//             "RTC rate was {}, must be a power of two between [2: 8192] inclusive!",
//             rate
//         );
//         return Err(());
//     }

//     // formula is "rate = 32768 Hz >> (dividor - 1)"
//     let dividor: u8 = log2(rate) as u8 + 2;

//     let _held_interrupts = hold_interrupts();

//     // bottom 4 bits of register A are the "rate dividor", setting them to rate we want without altering top 4 bits
//     write_cmos(0x8A);
//     let prev = read_cmos();
//     write_cmos(0x8A);

//     unsafe {
//         CMOS_WRITE_SETTINGS.lock().write((prev & 0xF0) | dividor);
//     }

//     trace!("RTC frequency changed to {} Hz!", rate);
//     Ok(())

//     // here: _held_interrupts falls out of scope, re-enabling interrupts if they were previously enabled.
// }

#[cfg(test)]
mod tests {
    use super::RtcTime;
    use crate::Duration;

    #[test]
    fn test_rtc_time_to_unix_time() {
        let time: Duration = RtcTime {
            seconds: 0,
            minutes: 0,
            hours: 0,
            days: 1,
            months: 1,
            years: 0,
        }
        .into();
        assert_eq!(time, Duration::from_secs(946_684_800));

        let time: Duration = RtcTime {
            seconds: 40,
            minutes: 46,
            hours: 1,
            days: 9,
            months: 9,
            years: 1,
        }
        .into();
        assert_eq!(time, Duration::from_secs(1_000_000_000));

        let time: Duration = RtcTime {
            seconds: 30,
            minutes: 31,
            hours: 23,
            days: 13,
            months: 2,
            years: 9,
        }
        .into();
        assert_eq!(time, Duration::from_secs(1_234_567_890));

        let time: Duration = RtcTime {
            seconds: 20,
            minutes: 33,
            hours: 3,
            days: 18,
            months: 5,
            years: 33,
        }
        .into();
        assert_eq!(time, Duration::from_secs(2_000_000_000));

        let time: Duration = RtcTime {
            seconds: 49,
            minutes: 6,
            hours: 9,
            days: 16,
            months: 6,
            years: 34,
        }
        .into();
        assert_eq!(time, Duration::from_secs(2_034_061_609));

        let time: Duration = RtcTime {
            seconds: 0,
            minutes: 20,
            hours: 5,
            days: 24,
            months: 1,
            years: 65,
        }
        .into();
        assert_eq!(time, Duration::from_secs(3_000_000_000));
    }
}
