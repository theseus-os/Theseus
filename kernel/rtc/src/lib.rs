#![no_std]
#![feature(unboxed_closures)]
#![feature(abi_x86_interrupt)]
#![feature(fn_traits)]

// extern crate alloc;
#[macro_use] extern crate lazy_static;
extern crate port_io;
extern crate irq_safety;
extern crate spin;
extern crate state_store;
#[macro_use] extern crate log;
extern crate x86_64;

use port_io::Port;
use irq_safety::hold_interrupts;
use core::sync::atomic::{AtomicUsize, Ordering};
use spin::Mutex;
// use spin::Once;
use state_store::{get_state, insert_state, SSCached};
use timer::Timespec;


//standard port to write to on CMOS to select registers
const CMOS_WRITE_PORT: u16 = 0x70;
//standard port to read register values from on CMOS or write to to change settings
const CMOS_READ_PORT: u16 = 0x71;

//used to select register
static CMOS_WRITE: Mutex<Port<u8>> = Mutex::new( Port::new(CMOS_WRITE_PORT));
//used to change cmos settings
static CMOS_WRITE_SETTINGS: Mutex<Port<u8>> = Mutex::new(Port::new(CMOS_READ_PORT));
//used to read from cmos register
static CMOS_READ: Mutex<Port<u8>> = Mutex::new( Port::new(CMOS_READ_PORT));


type RtcTicks = AtomicUsize;
lazy_static! {
    static ref RTC_TICKS: SSCached<RtcTicks> = {
        insert_state(RtcTicks::new(0));
        get_state::<RtcTicks>()
    };
}

pub type RtcInterruptFunction = fn(Option<usize>);

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

pub struct RtcTimer;

impl timer::Timer for RtcTimer {
    /// Time since 12:00am January 1st 1970 (i.e. Unix time).
    ///
    /// The algorithm is based on [IEEE 1003.1-2017 Base Definitions 4.16].
    ///
    /// [IEEE 1003.1-2017 Base Definitions 4.16]: https://pubs.opengroup.org/onlinepubs/9699919799.2018edition/
    fn value() -> Timespec {
        let time = read_rtc();
        time.into()
    }
}


//write a u8 to the CMOS port (0x70)
fn write_cmos(value: u8) {
    unsafe{
        CMOS_WRITE.lock().write(value)
    }
}


//read a u8 from CMOS port 0x71
fn read_cmos() -> u8{
    CMOS_READ.lock().read()
}



//returns true if update in progress, false otherwise
fn is_update_in_progress() -> bool{
    //writing to this register causes cmos to output 1 if rtc update in progress 
    write_cmos(0x0A);
    let is_in_progress: bool = read_cmos() == 1;
    is_in_progress
}


//register value is entered, rtc's associated value is output, waits for update in progress signal to end
fn read_register(register: u8) -> u8{
    
    //waits for "update in progress" signal to finish in order to read correct values
    while is_update_in_progress() {}
    write_cmos(register);

    //converts bcd value to binary value which is what is used for printing 
    let bcd = read_cmos();
    
    (bcd/16)*10 + (bcd & 0xf)
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

use core::fmt;
impl fmt::Display for RtcTime {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(f, "RTC Time: {}/{}/{} {}:{}:{}", 
            self.years, self.months, self.days, self.hours, self.minutes, self.seconds)
    }
}

impl From<RtcTime> for Timespec {
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

        const DAYS_IN_MONTH: [u16; 12] = [
            31,
            28,
            31,
            30,
            31,
            30,
            31,
            31,
            30,
            31,
            30,
            31,
        ];
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
        secs += ((years_since_1900 - 69)/4) * DAY_MULTIPLIER;
        // Subtracts a day back out every 100 years starting in 2001.
        secs += ((years_since_1900 - 1)/100) * DAY_MULTIPLIER;
        // Adds a day back in every 400 years starting in 2001.
        secs -= ((years_since_1900 + 299)/400) * DAY_MULTIPLIER;

        Timespec {
            secs,
            nanos: 0,
        }
    }
}

// Reads and returns the [`RtcTime`] from the RTC CMOS.
pub fn read_rtc() -> RtcTime {
    // Calls read register function which writes to port 0x70 to set RTC then reads from 0x71 which
    // outputs correct value
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

pub fn get_rtc_ticks() -> Result<usize, ()> {
     if let Some(ticks) = RTC_TICKS.get() {
         Ok(ticks.load(Ordering::Acquire))
     }
     else {
        Err(())
     }
}


/// turn on IRQ 8 (mapped to 0x28), rtc begins sending interrupts 
pub fn enable_rtc_interrupt()
{
    let _held_interrupts = hold_interrupts();

    write_cmos(0x0C);
    read_cmos();
    //select cmos register 0x8B
    write_cmos(0x8B);

    //value needed to turn on bit 6 of register B
    let prev = read_cmos();

    //we want it to go back to register 0x8B, it was reset when read
    write_cmos(0x8B);

    //here we don't use the cmos_write function because that only writes to port 0x70, in this case we need to write to 0x71
    //writing to 0x71 because not selecting register, setting rtc
    unsafe{
        CMOS_WRITE_SETTINGS.lock().write(prev | 0x40); 
    }

    trace!("RTC Enabled!");
    // here: _held_interrupts falls out of scope, re-enabling interrupts if they were previously enabled.
}


/// the log base 2 of an integer value
fn log2(value: usize) -> usize {
    let mut v = value;
    let mut result = 0;
    v >>= 1;
    while v > 0 {
        result += 1;
        v >>= 1;
    }

    result
}

/// sets the period of the RTC interrupt. 
/// `rate` must be a power of 2, between 2 and 8192 inclusive.
pub fn set_rtc_frequency(rate: usize) -> Result<(), ()> {
    if !(rate.is_power_of_two() && rate >= 2 && rate <= 8192) {
        error!("RTC rate was {}, must be a power of two between [2: 8192] inclusive!", rate);
        return Err(());
    }

    // formula is "rate = 32768 Hz >> (dividor - 1)"
    let dividor: u8 = log2(rate) as u8 + 2; 

    let _held_interrupts = hold_interrupts();

    // bottom 4 bits of register A are the "rate dividor", setting them to rate we want without altering top 4 bits
    write_cmos(0x8A);
    let prev = read_cmos();
    write_cmos(0x8A); 

    unsafe{
        CMOS_WRITE_SETTINGS.lock().write((prev & 0xF0) | dividor);
    }

    trace!("RTC frequency changed to {} Hz!", rate);
    Ok(())
    
    // here: _held_interrupts falls out of scope, re-enabling interrupts if they were previously enabled.
}

#[cfg(test)]
mod tests {
    use super::RtcTime;
    use timer::Timespec;

    #[test]
    fn test_rtc_time_to_unix() {
        let time: Timespec = RtcTime {
            seconds: 0,
            minutes: 0,
            hours: 0,
            days: 1,
            months: 1,
            years: 0,
        }
        .into();
        assert_eq!(
            time,
            Timespec {
                secs: 946_684_800,
                nanos: 0
            }
        );

        let time: Timespec = RtcTime {
            seconds: 40,
            minutes: 46,
            hours: 1,
            days: 9,
            months: 9,
            years: 1,
        }
        .into();
        assert_eq!(
            time,
            Timespec {
                secs: 1_000_000_000,
                nanos: 0
            }
        );

        let time: Timespec = RtcTime {
            seconds: 30,
            minutes: 31,
            hours: 23,
            days: 13,
            months: 2,
            years: 9,
        }
        .into();
        assert_eq!(
            time,
            Timespec {
                secs: 1_234_567_890,
                nanos: 0
            }
        );

        let time: Timespec = RtcTime {
            seconds: 20,
            minutes: 33,
            hours: 3,
            days: 18,
            months: 5,
            years: 33,
        }
        .into();
        assert_eq!(
            time,
            Timespec {
                secs: 2_000_000_000,
                nanos: 0
            }
        );

        let time: Timespec = RtcTime {
            seconds: 49,
            minutes: 6,
            hours: 9,
            days: 16,
            months: 6,
            years: 34,
        }
        .into();
        assert_eq!(
            time,
            Timespec {
                secs: 2_034_061_609,
                nanos: 0
            }
        );

        let time: Timespec = RtcTime {
            seconds: 0,
            minutes: 20,
            hours: 5,
            days: 24,
            months: 1,
            years: 65,
        }
        .into();
        assert_eq!(
            time,
            Timespec {
                secs: 3_000_000_000,
                nanos: 0
            }
        );
    }
}
