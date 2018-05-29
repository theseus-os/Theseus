#![no_std]
#![feature(alloc)]
#![feature(const_fn)]
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
use spin::{Mutex, Once};
use state_store::{get_state, insert_state, SSCached};


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

static RTC_INTERRUPT_FUNC: Once<RtcInterruptFunction> = Once::new();

/// Initialize the RTC interrupt with the given frequency
/// and the given closure that will run on each RTC interrupt.
/// The closure is provided with the current number of RTC ticks since boot,
/// in the form of an `Option<usize>` because it is not guaranteed that the number of ticks can be retrieved.
pub fn init(rtc_freq: usize, interrupt_func: RtcInterruptFunction) -> Result<(HandlerFunc), ()> {
    RTC_INTERRUPT_FUNC.call_once(|| interrupt_func);
    enable_rtc_interrupt();
    let res = set_rtc_frequency(rtc_freq);
    res.map( |_| rtc_interrupt_handler as HandlerFunc )
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
    pub seconds: u8,
    pub minutes: u8,
    pub hours: u8,
    pub days: u8,
    pub months: u8,
    pub years: u8,
}
use core::fmt;
impl fmt::Display for RtcTime {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(f, "RTC Time: {}/{}/{} {}:{}:{}", 
            self.years, self.months, self.days, self.hours, self.minutes, self.seconds)
    }
}

//call this function to print RTC's date and time
pub fn read_rtc() -> RtcTime {

    //calls read register function which writes to port 0x70 to set RTC then reads from 0x71 which outputs correct value
    let second = read_register(0x00);
    let minute = read_register(0x02);
    let hour = read_register(0x04);
    let day = read_register(0x07);
    let month = read_register(0x08);
    let year = read_register(0x09);

    RtcTime {
        seconds: second, 
        minutes: minute, 
        hours: hour, 
        days: day, 
        months: month, 
        years: year
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
fn enable_rtc_interrupt()
{
    // disable interrupts until _held_interrupts goes out of scope
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
fn set_rtc_frequency(rate: usize) -> Result<(), ()> {

    if (!rate.is_power_of_two()) || rate < 2 || rate > 8192 {
        error!("RTC rate was {}, must be a power of two between [2: 8192] inclusive!", rate);
        return Err(());
    }

    // disable interrupts until _held_interrupts goes out of scope
    let _held_interrupts = hold_interrupts();

    // formula is "rate = 32768 Hz >> (dividor - 1)"
    let dividor: u8 = log2(rate) as u8 + 2; 

    //bottom 4 bits of register A are rate, setting them to rate we want without altering top 4 bits
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


// idea here: https://github.com/rust-lang/rust/issues/44291

use x86_64::instructions::port::outb;
use x86_64::structures::idt::{HandlerFunc, ExceptionStackFrame};
// use alloc::boxed::Box;

// pub fn rtc_interrupt_handler_builder<'a, F>(func: F) -> &'a Fn()
//     // ( extern "x86-interrupt" fn(&mut ExceptionStackFrame) )
//     where F: Fn(Option<usize>) + 'a
// {
//     let returned_func = || {
//         // writing to register 0x0C and reading its value is required for subsequent interrupts to fire
//         write_cmos(0x0C);
//         read_cmos();

//         let arg: Option<usize> = RTC_TICKS.get().map(|atomic_ticks| atomic_ticks.fetch_add(1, Ordering::SeqCst) + 1); // +1 because fetch_add returns previous value
//         // because the RTC handler is used for scheduling, we have to acknowledge the interrupt now
//         unsafe { outb(0x20, 0x20); outb(0xA0, 0x20); }
//         func(arg);

//     };

//     &'a returned_func
// }



// pub fn rtc_interrupt_handler_builder(func: Box<Fn(Option<usize>)> ) -> ( extern "x86-interrupt" fn(&mut ExceptionStackFrame) )
// {
//     // let closure = || {
//     // };

//     let returned_func = |esf| {
//         // writing to register 0x0C and reading its value is required for subsequent interrupts to fire
//         write_cmos(0x0C);
//         read_cmos();

//         let arg: Option<usize> = RTC_TICKS.get().map(|atomic_ticks| atomic_ticks.fetch_add(1, Ordering::SeqCst) + 1); // +1 because fetch_add returns previous value
//         // because the RTC handler is used for scheduling, we have to acknowledge the interrupt now
//         unsafe { outb(0x20, 0x28); outb(0xA0, 0x28); }
//         func(arg);

//     };

//     returned_func as (extern "x86-interrupt" fn(&mut ExceptionStackFrame))
// }



pub fn rtc_ack_irq() {
    // writing to register 0x0C and reading its value is required for subsequent interrupts to fire
    write_cmos(0x0C);
    read_cmos();
}


pub extern "x86-interrupt" fn rtc_interrupt_handler(_stack_frame: &mut ExceptionStackFrame) {

    // we must acknowledge the interrupt first before handling it, in case the custom func does something like context switch
    rtc_ack_irq();
    unsafe { outb(0x20, 0x20); outb(0xA0, 0x20); } // equivalent to:  unsafe { PIC.notify_end_of_interrupt(0x28); } 

    if let Some(rtc_interrupt_func) = RTC_INTERRUPT_FUNC.try() {
        let arg: Option<usize> = RTC_TICKS.get().map(|atomic_ticks| atomic_ticks.fetch_add(1, Ordering::SeqCst) + 1); // +1 because fetch_add returns previous value
        rtc_interrupt_func(arg);
    }
    else {
        warn!("RTC_INTERRUPT_FUNC was None, doing nothing this interrupt.");
    }
}
