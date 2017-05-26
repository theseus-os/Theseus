//can be used to read and write to ports, look in docs.rs for details. 
//extern crate cpuio;
//access to registers? x86_64 crate is probably needed but have to find which  part is relevant
use x86_64::registers;
use spin::{Mutex, Once};
use port_io::InOut;



pub fn read_rtc(){
    
    //initializing variables to be read from RTC
    let mut second: u64 = 0;
    let mut minute: u64 = 0;
    let mut hour: u64 = 0;
    let mut day: u64 = 0;
    let mut month: u64 = 0;
    let mut year: u64 = 0;

    //set register
    InOut::port_out(0x70, 0x02);
    //read register
    second = InOut::port_in(0x71: u8);


}

