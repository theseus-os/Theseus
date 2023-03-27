//! Full support for the Programmable Interval Timer (PIT) system clock.
//! 
//! This crate allows one to enable/configure PIT interrupts.
//! For a simpler crate that just allows PIT-based waiting, use `pit_clock_basic`.

#![no_std]
#![feature(abi_x86_interrupt)]

#[macro_use] extern crate log;
extern crate port_io;
extern crate pit_clock_basic;
extern crate interrupts;
extern crate x86_64;

use port_io::Port;
use core::sync::atomic::{AtomicUsize, Ordering};
use x86_64::structures::idt::InterruptStackFrame;

pub use pit_clock_basic::pit_wait;
use pit_clock_basic::*;

/// The PIT Channel 0 is connected directly to IRQ 0.
/// Because we perform the typical PIC remapping, the remapped IRQ vector number is 0x20.
#[allow(clippy::identity_op)]
const PIT_CHANNEL_0_IRQ: u8 = interrupts::IRQ_BASE_OFFSET + 0x0;


/// Configures the PIT to fire an interrupt at the given frequency (in Hz).
/// 
/// Only Channel 0 of the PIT is directly connected to an IRQ, so it must be used.
/// Note that Channel 1 does not exist and Channel 2 is reserved for non-interrupt timer usage.
/// 
/// ## Arguments
/// * `freq_hertz`: the frequency in Hz of the desired PIT interrupt.
///    The minimum value is 19 Hz, which is based on the fact that the timer register
///    cannot be loaded with a value larger than `u16::MAX` (65535),
///    and that the value loaded into the register is a divisor value.
///    That divisor value is the default timer frequency 1193182 divided by `freq_hertz`.
pub fn enable_interrupts(freq_hertz: u32) -> Result<(), &'static str> {
    let divisor = PIT_DEFAULT_DIVIDEND_HZ / freq_hertz;
    if divisor > u16::MAX as u32 {
        error!("The chosen PIT frequency ({} Hz) is too small, it must be {} Hz or greater!", 
            freq_hertz, PIT_MINIMUM_FREQ
        );
        return Err("The chosen PIT frequency is too small, it must be 19 Hz or greated")
    }

    // Register the interrupt handler
    match interrupts::register_interrupt(PIT_CHANNEL_0_IRQ, pit_timer_handler) {
        Ok(_) => { /* success, do nothing */}
        Err(handler) if handler == pit_timer_handler as usize => { /* already registered, do nothing */ }
        Err(_other) => return Err(" PIT clock IRQ was already in use; sharing IRQs is currently unsupported"),
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

    Ok(())
}


extern "x86-interrupt" fn pit_timer_handler(_stack_frame: InterruptStackFrame) {
    static PIT_TICKS: AtomicUsize = AtomicUsize::new(0);
    let ticks = PIT_TICKS.fetch_add(1, Ordering::Acquire);
    trace!("PIT timer interrupt, ticks: {}", ticks);

    interrupts::eoi(Some(PIT_CHANNEL_0_IRQ));
}
