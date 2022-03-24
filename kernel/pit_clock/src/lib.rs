//! Support for the Programmable Interval Timer (PIT) system clock,
//! which enables and configures the frequency of simple timer interrupts
//! and basic timer-based one-shot wait period.

#![no_std]

extern crate spin;
#[macro_use] extern crate log;
extern crate port_io;

use port_io::Port;
use spin::Mutex;
use core::sync::atomic::{AtomicUsize, Ordering};


/// the main interrupt channel
const CHANNEL0: u16 = 0x40;
/// DO NOT USE
const _CHANNEL1: u16 = 0x41;
/// the speaker for beeping.
/// also used for one-shot waits.
const CHANNEL2: u16 = 0x42;

const COMMAND_REGISTER: u16 = 0x43;


/// the timer's default frequency is 1.19 MHz
const PIT_DEFAULT_DIVIDEND_HZ: u32 = 1193182;
const PIT_MINIMUM_FREQ: u32 = 19;

static PIT_COMMAND: Mutex<Port<u8>> = Mutex::new( Port::new(COMMAND_REGISTER) );
static PIT_CHANNEL_0: Mutex<Port<u8>> = Mutex::new( Port::new(CHANNEL0) );
static PIT_CHANNEL_2: Mutex<Port<u8>> = Mutex::new( Port::new(CHANNEL2) );

static PIT_TICKS: AtomicUsize = AtomicUsize::new(0);


pub fn init(freq_hertz: u32) {
    let divisor = PIT_DEFAULT_DIVIDEND_HZ / freq_hertz;
    if divisor > (u16::max_value() as u32) {
        panic!("The chosen PIT frequency ({} Hz) is too small, it must be {} Hz or greater!", 
                freq_hertz, PIT_MINIMUM_FREQ);
    }

    // SAFE because we're simply configuring the PIT clock, and the code below is correct.
    unsafe {
        PIT_COMMAND.lock().write(0x36); // 0x36: see this: http://www.osdever.net/bkerndev/Docs/pit.htm

        // must write the low byte and then the high byte
        PIT_CHANNEL_0.lock().write(divisor as u8);
        // read from PS/2 port 0x60, which acts as a short delay and acknowledges the status register
        let _ignore: u8 = Port::new(0x60).read();
        PIT_CHANNEL_0.lock().write((divisor >> 8) as u8);
    }
}


/// Wait a certain number of microseconds, max 55555 microseconds.
/// Uses a separate PIT clock channel, so it doesn't affect the regular PIT interrupts on PIT channel 0.
pub fn pit_wait(microseconds: u32) -> Result<(), &'static str> {
    let divisor = PIT_DEFAULT_DIVIDEND_HZ / (1000000/microseconds); 
    if divisor > (u16::max_value() as u32) {
        error!("pit_wait(): the chosen wait time {}us is too large, max value is {}!", microseconds, 1000000/PIT_MINIMUM_FREQ);
        return Err("microsecond value was too large");
    }

    // SAFE because we're simply configuring the PIT clock, and the code below is correct.
    unsafe {
        let port_60 = Port::<u8>::new(0x60);
        let port_61 = Port::<u8>::new(0x61);

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
        while port_61.read() & 0x20 != 0 { }
        Ok(())
    }
}

/// this occurs on every PIT timer tick.
/// Called by the PIT's interrupt handler
pub fn handle_timer_interrupt() {
    let ticks = PIT_TICKS.fetch_add(1, Ordering::Acquire);
    trace!("PIT timer interrupt, ticks: {}", ticks);
}
