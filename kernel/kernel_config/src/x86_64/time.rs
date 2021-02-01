

/// the chosen interrupt frequency (in Hertz) of the PIT clock 
pub const CONFIG_PIT_FREQUENCY_HZ: u32 = 1000; 

/// the chosen interrupt frequency (in Hertz) of the RTC.
/// valid values are powers of 2, from 2 Hz up to 8192 Hz
/// see [change_rtc_frequency()](rtc/)
pub const CONFIG_RTC_FREQUENCY_HZ: usize = 128;

/// The timeslice period, specified in microseconds.
pub const CONFIG_TIMESLICE_PERIOD_MICROSECONDS: u32 = 8000; // 8ms

/// the heartbeat period in milliseconds
pub const CONFIG_HEARTBEAT_PERIOD_MS: usize = 10000;