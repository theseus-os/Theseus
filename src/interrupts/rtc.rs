use port_io::Port;

//write a u8 to the CMOS port (0x70)
fn write_cmos(value: u8){
    let mut cmos_write: Port<u8> = unsafe { Port::new(0x70)};
    unsafe{cmos_write.write(value)};

}

//read a u8 from CMOS port 0x71
fn read_cmos()->u8{
    let mut cmos_read: Port<u8> = unsafe { Port::new(0x71)};
    let read_value: u8 = cmos_read.read();
    read_value
}



//let mut cmos_read: Port<u8> = unsafe { Port::new(0x71)};
//returns true if update in progress, false otherwise
fn get_update_in_progress()-> bool{
    
    write_cmos(0x0A);
    let is_in_progress: bool = read_cmos() == 1;
    is_in_progress


}

//register value is entered, rtc's associated value is output, waits for update in progress signal to end
fn read_register(register: u8)->u8{
    
    //waits for "update in progress" signal to finish in order to read correct values
    while get_update_in_progress() {}
    write_cmos(register);

    //converts bcd value to binary value which is what is used for output
    let bcd = read_cmos();
    let bin_mode: u8  = (bcd/16)*10 + (bcd & 0xf);
    bin_mode

}


//call this function to print RTC's date and time
pub fn read_rtc(){

    //calls read register function which writes to port 0x70 to set RTC then reads from 0x71 which outputs correct value
    let seconds = read_register(0x00);
    let minutes = read_register(0x02);
    let hour = read_register(0x04);
    let day = read_register(0x07);
    let month = read_register(0x08);
    let year = read_register(0x09);

    
    trace!("Time - {}:{}:{} {}/{}/{}", hour, minutes,seconds, month, day, year);


}

