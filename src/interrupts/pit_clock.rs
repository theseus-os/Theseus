/// Handles the Programmable Interval Timer (PIT) system clock,
/// which allows us to configure the frequency of timer interrupts. 

use port_io::Port;
use spin::Mutex;
use core::sync::atomic::{AtomicUsize, Ordering};
use interrupts::time_tools;

/// the main interrupt channel
const CHANNEL0: u16 = 0x40;
/// DO NOT USE
const CHANNEL1: u16 = 0x41;
/// the speaker for beeping
const CHANNEL2: u16 = 0x42;

const COMMAND_REGISTER: u16 = 0x43;


/// the timer's default frequency is 1.19 MHz
const PIT_DIVIDEND_HZ: u32 = 1193182; 

static PIT_COMMAND: Mutex<Port<u8>> = Mutex::new( Port::new(COMMAND_REGISTER) );
static PIT_CHANNEL_0: Mutex<Port<u8>> = Mutex::new( Port::new(CHANNEL0) );


// static TICKS: AtomicUsize = AtomicUsize::new(0); // default()??
pub static mut TICKS: u64 = 0;


pub fn init(freq_hertz: u32) {
    let divisor = PIT_DIVIDEND_HZ / freq_hertz;
    if divisor > (u16::max_value() as u32) {
        panic!("The chosen PIT frequency ({} Hz) is too small, it must be higher than 18 Hz!", freq_hertz);
    }

    // SAFE because we're simply configuring the PIT clock, and the code below is correct.
    unsafe {
        PIT_COMMAND.lock().write(0x36); // 0x36: see this: http://www.osdever.net/bkerndev/Docs/pit.htm
        // PIT_COMMAND.lock().write(0x34); // some other mode that we don't want

        // must write the low byte and then the high byte
        PIT_CHANNEL_0.lock().write(divisor as u8);
        PIT_CHANNEL_0.lock().write((divisor >> 8) as u8);
    }
}


// these should be moved to a config file

/// the chosen interrupt frequency (in Hertz) of the PIT clock 
const PIT_FREQUENCY_HZ: u64 = 100; 

/// the timeslice period in milliseconds
const timeslice_period_ms: u64 = 10; 

/// the heartbeatperiod in milliseconds
const heartbeat_period_ms: u64 = 1000;

/// this occurs on every PIT timer tick, which is currently 100 Hz
pub fn handle_timer_interrupt() {
    let ticks = unsafe {
        TICKS += 1;
        TICKS
    };


    if (ticks % (timeslice_period_ms * PIT_FREQUENCY_HZ / 1000)) == 0 {
        schedule!();
//<<<<<<< HEAD

//=======
        // trace!("done with preemptive schedule call (ticks={})", ticks);
//>>>>>>> 9cf36e87f1abf2d0aee29c27599cd978741167db
    }

    if (ticks % (heartbeat_period_ms * PIT_FREQUENCY_HZ / 1000)) == 0 {
        
        //initializing TimeKeeping struct to "count"" ticks passed
        let mut test: time_tools::TimeKeeping = time_tools::TimeKeeping{start_time:ticks, end_time: 9000};

        trace!("[heartbeat] {} seconds have passed (ticks={})", heartbeat_period_ms/1000, ticks);
        

        test.end_time = time_tools::get_ticks();
        trace!("[tester]{} ticks passed during heartbeat statement({} = starting number), ({} = ending number)" , test.end_time-test.start_time, test.start_time, test.end_time);
        //time_tools::return_ticks();
        // info!("1 second has passed (ticks={})", ticks);
    }
}