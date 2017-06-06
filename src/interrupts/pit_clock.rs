/// Handles the Programmable Interval Timer (PIT) system clock,
/// which allows us to configure the frequency of timer interrupts. 

use port_io::Port;
use spin::Mutex;
use core::sync::atomic::{AtomicUsize, Ordering};


/// the main interrupt channel
const CHANNEL0: u16 = 0x40;
/// DO NOT USE
const _CHANNEL1: u16 = 0x41;
/// the speaker for beeping
const _CHANNEL2: u16 = 0x42;

const COMMAND_REGISTER: u16 = 0x43;


/// the timer's default frequency is 1.19 MHz
const PIT_DEFAULT_DIVIDEND_HZ: u32 = 1193182; 

static PIT_COMMAND: Mutex<Port<u8>> = Mutex::new( Port::new(COMMAND_REGISTER) );
static PIT_CHANNEL_0: Mutex<Port<u8>> = Mutex::new( Port::new(CHANNEL0) );


pub static PIT_TICKS: AtomicUsize = AtomicUsize::new(0);


pub fn init(freq_hertz: u32) {
    let divisor = PIT_DEFAULT_DIVIDEND_HZ / freq_hertz;
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



/// this occurs on every PIT timer tick
pub fn handle_timer_interrupt() {
    PIT_TICKS.fetch_add(1, Ordering::SeqCst);
}