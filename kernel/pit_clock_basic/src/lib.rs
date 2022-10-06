//! Basic support for waiting using the Programmable Interval Timer (PIT) clock.
//! 
//! Use the `pit_clock` crate for a more fully-featured PIT interface,
//! including enabling interrupts.

#![no_std]

extern crate spin;
#[macro_use] extern crate log;
extern crate port_io;

use port_io::Port;
use spin::Mutex;

/// Port for Channel 0; used for PIT interrupts.
pub const CHANNEL0: u16 = 0x40;
/// Port for Channel 1, which does not exist and should NOT be used.
const _CHANNEL1: u16 = 0x41;
/// Port for Channel 2; technically the speaker for beeps,
/// but is also used for waiting in `pit_wait()`.
const CHANNEL2: u16 = 0x42;
/// Port for the PIT command register. 
const COMMAND_REGISTER: u16 = 0x43;

/// the timer's default frequency is 1.19 MHz
pub const PIT_DEFAULT_DIVIDEND_HZ: u32 = 1193182;
pub const PIT_MINIMUM_FREQ:        u32 = 19;

pub static PIT_COMMAND:   Mutex<Port<u8>> = Mutex::new( Port::new(COMMAND_REGISTER) );
pub static PIT_CHANNEL_0: Mutex<Port<u8>> = Mutex::new( Port::new(CHANNEL0) );
static PIT_CHANNEL_2: Mutex<Port<u8>> = Mutex::new( Port::new(CHANNEL2) );


/// Waits (blocking) for the given number of `microseconds` using the PIT Channel 2.
/// 
/// This uses a separate PIT clock channel so it doesn't affect PIT interrupts.
/// 
/// ## Arguments
/// * `microseconds`: the number of microseconds to wait, max value 55555.
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
        
        // wait for PIT timer to reach 0, which is tested by checking bit 5
        while port_61.read() & 0x20 != 0 { }
        Ok(())
    }
}
