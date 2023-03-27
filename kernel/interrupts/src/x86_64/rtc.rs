use port_io::Port;
use core::sync::atomic::{AtomicUsize, Ordering};
pub use irq_safety::{hold_interrupts, enable_interrupts, interrupts_enabled};
use util;
use kernel_config::time::{CONFIG_TIMESLICE_PERIOD_MS, CONFIG_RTC_FREQUENCY_HZ};
use spin::Mutex;

//standard port to write to on CMOS to select registers
const CMOS_WRITE_PORT: u16 = 0x70;
//standard port to read register values from on CMOS or write to to change settings
const CMOS_READ_PORT: u16 = 0x71;

pub static RTC_TICKS: AtomicUsize = AtomicUsize::new(0);
//used to select register
static CMOS_WRITE: Mutex<Port<u8>> = Mutex::new( Port::new(CMOS_WRITE_PORT));
//used to change cmos settings
static CMOS_WRITE_SETTINGS: Mutex<Port<u8>> = Mutex::new(Port::new(CMOS_READ_PORT));
//used to read from cmos register
static CMOS_READ: Mutex<Port<u8>> = Mutex::new( Port::new(CMOS_READ_PORT));


//write a u8 to the CMOS port (0x70)
fn write_cmos(value: u8){

    unsafe{CMOS_WRITE.lock().write(value)}

}


//read a u8 from CMOS port 0x71
fn read_cmos()->u8{
    
    CMOS_READ.lock().read()
    
}



//returns true if update in progress, false otherwise
fn get_update_in_progress()-> bool{
    
    //writing to this register causes cmos to output 1 if rtc update in progress 
    write_cmos(0x0A);
    let is_in_progress: bool = read_cmos() == 1;
    is_in_progress

}


//register value is entered, rtc's associated value is output, waits for update in progress signal to end
fn read_register(register: u8)->u8{
    
    //waits for "update in progress" signal to finish in order to read correct values
    while get_update_in_progress() {}
    write_cmos(register);

    //converts bcd value to binary value which is what is used for printing 
    let bcd = read_cmos();
    
    (bcd/16)*10 + (bcd & 0xf)


}

pub struct time{
    seconds: u8,
    minutes: u8,
    hours: u8,
    days: u8,
    months: u8,
    years: u8,

}

//call this function to print RTC's date and time
pub fn read_rtc()->time{

    //calls read register function which writes to port 0x70 to set RTC then reads from 0x71 which outputs correct value
    let second = read_register(0x00);
    let minute = read_register(0x02);
    let hour = read_register(0x04);
    let day = read_register(0x07);
    let month = read_register(0x08);
    let year = read_register(0x09);

    
    trace!("Time - {}:{}:{} {}/{}/{}", hour, minute,second, month, day, year);

    time{seconds:second, minutes: minute, hours: hour, days: day, months: month, years: year}

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
    
    unsafe{CMOS_WRITE_SETTINGS.lock().write(prev | 0x40)};

    trace!("RTC Enabled!");
}


/// the heartbeatperiod in milliseconds
const heartbeat_period_ms: u64 = 1000;

/// changes the period of the RTC interrupt. 
/// `rate` must be a power of 2, between 2 and 8192 inclusive.
pub fn change_rtc_frequency(rate: usize) {

    let ispow2: bool = rate.is_power_of_two();

    if (!rate.is_power_of_two()) || rate < 2 || rate > 8192 {
        panic!("RTC rate was {}, must be a power of two between [2: 8192]");
    }

    let _held_interrupts = hold_interrupts();
    
    // formula is "rate = 32768 Hz >> (dividor - 1)"
    let dividor: u8 = rate.ilog2() as u8 + 2; 

    //bottom 4 bits of register A are rate, setting them to rate we want without altering top 4 bits
    write_cmos(0x8A);
    let prev = read_cmos();
    write_cmos(0x8A); 

    unsafe{CMOS_WRITE_SETTINGS.lock().write(((prev & 0xF0)|dividor))};

    trace!("rtc rate frequency changed!");
}


pub fn rtc_ack_irq() {
    // writing to register 0x0C and reading its value is required for subsequent interrupts to fire
    write_cmos(0x0C);
    read_cmos();
}

/// counts interrupts from RTC
pub fn handle_rtc_interrupt() {
    // writing to register 0x0C and reading its value is required for subsequent interrupts to fire
    // rtc_ack_irq();  // FIXME: currently doing this in interrupts/mod.rs instead as a test

    let ticks = RTC_TICKS.fetch_add(1, Ordering::SeqCst) + 1; // +1 because fetch_add returns previous value

    
    if (ticks % (CONFIG_TIMESLICE_PERIOD_MS * CONFIG_RTC_FREQUENCY_HZ / 1000)) == 0 {
        schedule!();
    }


}