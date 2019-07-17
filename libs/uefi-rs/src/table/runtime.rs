//! UEFI services available at runtime, even after the OS boots.

use super::Header;
use crate::{Result, Status};
use bitflags::bitflags;
use core::mem;
use core::ptr;

/// Contains pointers to all of the runtime services.
///
/// This table, and the function pointers it contains are valid
/// even after the UEFI OS loader and OS have taken control of the platform.
#[repr(C)]
pub struct RuntimeServices {
    header: Header,
    get_time:
        unsafe extern "win64" fn(time: &mut Time, capabilities: *mut TimeCapabilities) -> Status,
    set_time: unsafe extern "win64" fn(time: &Time) -> Status,
    // Skip some useless functions.
    _pad: [usize; 8],
    reset: unsafe extern "win64" fn(
        rt: ResetType,
        status: Status,
        data_size: usize,
        data: *const u8,
    ) -> !,
}

impl RuntimeServices {
    /// Query the current time and date information
    pub fn get_time(&self) -> Result<Time> {
        let mut time = unsafe { mem::uninitialized() };
        unsafe { (self.get_time)(&mut time, ptr::null_mut()) }.into_with_val(|| time)
    }

    /// Query the current time and date information and the RTC capabilities
    pub fn get_time_and_caps(&self) -> Result<(Time, TimeCapabilities)> {
        let mut time = unsafe { mem::uninitialized() };
        let mut caps = unsafe { mem::uninitialized() };
        unsafe { (self.get_time)(&mut time, &mut caps as *mut _) }.into_with_val(|| (time, caps))
    }

    /// Sets the current local time and date information
    ///
    /// During runtime, if a PC-AT CMOS device is present in the platform, the
    /// caller must synchronize access to the device before calling set_time.
    pub unsafe fn set_time(&mut self, time: &Time) -> Result {
        (self.set_time)(time).into()
    }

    /// Resets the computer.
    pub fn reset(&self, rt: ResetType, status: Status, data: Option<&[u8]>) -> ! {
        let (size, data) = match data {
            // FIXME: The UEFI spec states that the data must start with a NUL-
            //        terminated string, which we should check... but it does not
            //        specify if that string should be Latin-1 or UCS-2!
            //
            //        PlatformSpecific resets should also insert a GUID after the
            //        NUL-terminated string.
            Some(data) => (data.len(), data.as_ptr()),
            None => (0, ptr::null()),
        };

        unsafe { (self.reset)(rt, status, size, data) }
    }
}

impl super::Table for RuntimeServices {
    const SIGNATURE: u64 = 0x5652_4553_544e_5552;
}

/// The current time information
#[derive(Debug, Copy, Clone)]
#[repr(C)]
pub struct Time {
    year: u16,  // 1900 - 9999
    month: u8,  // 1 - 12
    day: u8,    // 1 - 31
    hour: u8,   // 0 - 23
    minute: u8, // 0 - 59
    second: u8, // 0 - 59
    _pad1: u8,
    nanosecond: u32, // 0 - 999_999_999
    time_zone: i16,  // -1440 to 1440, or 2047 if unspecified
    daylight: Daylight,
    _pad2: u8,
}

bitflags! {
    /// Flags describing the capabilities of a memory range.
    pub struct Daylight: u8 {
        /// Time is affected by daylight savings time
        const ADJUST_DAYLIGHT = 0x01;
        /// Time has been adjusted for daylight savings time
        const IN_DAYLIGHT = 0x02;
    }
}

impl Time {
    /// Build an UEFI time struct
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        year: u16,
        month: u8,
        day: u8,
        hour: u8,
        minute: u8,
        second: u8,
        nanosecond: u32,
        time_zone: i16,
        daylight: Daylight,
    ) -> Self {
        assert!(year >= 1900 && year <= 9999);
        assert!(month >= 1 && month <= 12);
        assert!(day >= 1 && day <= 31);
        assert!(hour <= 23);
        assert!(minute <= 59);
        assert!(second <= 59);
        assert!(nanosecond <= 999_999_999);
        assert!((time_zone >= -1440 && time_zone <= 1440) || time_zone == 2047);
        Self {
            year,
            month,
            day,
            hour,
            minute,
            second,
            _pad1: 0,
            nanosecond,
            time_zone,
            daylight,
            _pad2: 0,
        }
    }

    /// Query the year
    pub fn year(&self) -> u16 {
        self.year
    }

    /// Query the month
    pub fn month(&self) -> u8 {
        self.month
    }

    /// Query the day
    pub fn day(&self) -> u8 {
        self.day
    }

    /// Query the hour
    pub fn hour(&self) -> u8 {
        self.hour
    }

    /// Query the minute
    pub fn minute(&self) -> u8 {
        self.minute
    }

    /// Query the second
    pub fn second(&self) -> u8 {
        self.second
    }

    /// Query the nanosecond
    pub fn nanosecond(&self) -> u32 {
        self.nanosecond
    }

    /// Query the time offset in minutes from UTC, or None if using local time
    pub fn time_zone(&self) -> Option<i16> {
        if self.time_zone == 2047 {
            None
        } else {
            Some(self.time_zone)
        }
    }

    /// Query the daylight savings time information
    pub fn daylight(&self) -> Daylight {
        self.daylight
    }
}

/// Real time clock capabilities
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[repr(C)]
pub struct TimeCapabilities {
    /// Reporting resolution of the clock in counts per second. 1 for a normal
    /// PC-AT CMOS RTC device, which reports the time with 1-second resolution.
    pub resolution: u32,

    /// Timekeeping accuracy in units of 1e-6 parts per million.
    pub accuracy: u32,

    /// Whether a time set operation clears the device's time below the
    /// "resolution" reporting level. False for normal PC-AT CMOS RTC devices.
    pub sets_to_zero: bool,
}

/// The type of system reset.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[repr(u32)]
pub enum ResetType {
    /// Resets all the internal circuitry to its initial state.
    ///
    /// This is analogous to power cycling the device.
    Cold = 0,
    /// The processor is reset to its initial state.
    Warm,
    /// The components are powered off.
    Shutdown,
    /// A platform-specific reset type.
    ///
    /// The additional data must be a pointer to
    /// a null-terminated string followed by an UUID.
    PlatformSpecific,
    // SAFETY: This enum is never exposed to the user, but only fed as input to
    //         the firmware. Therefore, unexpected values can never come from
    //         the firmware, and modeling this as a Rust enum seems safe.
}
